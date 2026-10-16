using System.Diagnostics;
using System.Runtime.CompilerServices;
using System.Text;

internal sealed class Workload : IDisposable
{
    private const int MiB = 1024 * 1024;
    private readonly Options o;
    private readonly Evidence evidence;
    private readonly List<nint> memory = [], windows = [], brushes = [];
    private readonly List<EventWaitHandle> handles = [];
    private readonly List<Thread> threads = [];
    private readonly ManualResetEvent stop = new(false);
    private readonly List<FileStream> files = [];
    private readonly List<byte[]> gen2 = [], loh = [], poh = [];
    private readonly byte[] ioBuffer = new byte[MiB];
    private readonly Stopwatch time = Stopwatch.StartNew();
    private string? scratch;
    private FileStream? io;
    private nint gpu;
    private volatile int cpuDuty;
    private volatile Exception? workerFailure;
    private bool disposed;
    private bool touched;
    private int retainedLevel = -1, targetRate;
    private double lastTick, lastGpuTick = -1;
    private long transferred, allocated, cpuIterations, gpuPasses;
    private static volatile object? allocationSink;
    public Workload(Options options, Evidence evidence) { o = options; this.evidence = evidence; }
    private int Scaled(int value) => Math.Max(1, (int)(value * o.Scale));
    private bool Is(string name) => o.Workload == name;
    private bool IsDotnet => o.Workload.StartsWith("dotnet", StringComparison.Ordinal);
    private bool IsGpu => o.Workload.StartsWith("gpu", StringComparison.Ordinal);
    private int Retained(Phase phase) => o.Mode == "screenshot" && phase.Level > 0 ? (phase.MemoryStage == 3 ? 2 : 1) : phase.Level;

    public void Apply(Phase phase)
    {
        int level = Retained(phase);
        if (Is("cpu"))
        {
            if (threads.Count == 0) StartThreads(o.CpuWorkers, true);
            cpuDuty = phase.Level == 0 ? 0 : phase.Level == 1 ? 50 : 100;
        }
        if (Is("memory")) SetMemory(phase.MemoryStage);
        if (Is("threads") && level != retainedLevel)
        {
            StopThreads();
            StartThreads(level == 0 ? 0 : Scaled(level == 1 ? 8 : 32), false);
        }
        if (Is("handles")) Resize(handles, level == 0 ? 0 : Scaled(level == 1 ? 200 : 1000),
            () => new EventWaitHandle(false, EventResetMode.ManualReset), h => h.Dispose());
        if (Is("files"))
        {
            EnsureScratch();
            Resize(files, level == 0 ? 0 : Scaled(level == 1 ? 4 : 16),
                () => new FileStream(Path.Combine(scratch!, $"held-{files.Count:D2}.dat"), FileMode.OpenOrCreate,
                    FileAccess.ReadWrite, FileShare.ReadWrite | FileShare.Delete), f => f.Dispose());
        }
        if (Is("io-read") || Is("io-write"))
        {
            EnsureScratch();
            if (io == null)
            {
                io = new FileStream(Path.Combine(scratch!, "transfer.dat"), FileMode.CreateNew, FileAccess.ReadWrite, FileShare.Read,
                    4096, FileOptions.None);
                // Prepare a fixed 64 MiB file before timing the requested workload.
                for (int i = 0; i < 64; ++i) io.Write(ioBuffer);
                io.Flush(true); io.Position = 0;
                evidence.Write("setup", new { file_bytes = io.Length, preparation_write_bytes = io.Length });
            }
            targetRate = phase.Level == 0 ? 0 : Scaled(phase.Level == 1 ? 8 * MiB : 32 * MiB);
        }
        if ((Is("objects") || Is("user"))) Resize(windows, level == 0 ? 0 : Scaled(level == 1 ? 10 : 50),
            () => Native.Require(Native.CreateWindowExW(0, "STATIC", "Resource workload", 0, 0, 0, 1, 1, 0, 0, 0, 0)),
            h => Native.Require(Native.DestroyWindow(h)));
        if ((Is("objects") || Is("gdi"))) Resize(brushes, level == 0 ? 0 : Scaled(level == 1 ? 100 : 500),
            () => Native.Require(Native.CreateSolidBrush(0x806040)), h => Native.Require(Native.DeleteObject(h)));
        if (IsDotnet)
        {
            if (level != retainedLevel)
            {
                int g = level == 0 ? 0 : Scaled(level == 1 ? 64 : 192);
                int l = level == 0 ? 0 : Scaled(level == 1 ? 64 : 256);
                int p = level == 0 ? 0 : Scaled(level == 1 ? 32 : 128);
                SetManaged(gen2, Is("dotnet") || Is("dotnet-gen2") ? g : 0, 16 * 1024, false);
                SetManaged(loh, Is("dotnet") || Is("dotnet-loh") || Is("dotnet-frag") ? l : 0, MiB, false);
                SetManaged(poh, Is("dotnet") || Is("dotnet-poh") ? p : 0, MiB, true);
                if (Is("dotnet-frag") && level == 2)
                    for (int i = loh.Count - 2; i >= 0; i -= 2) loh.RemoveAt(i);
                if (o.ForceGc) Collect("retention-transition");
            }
            targetRate = (Is("dotnet") || Is("dotnet-short")) && phase.Level > 0 ? Scaled(phase.Level == 1 ? 8 * MiB : 32 * MiB) : 0;
        }
        if (IsGpu)
        {
            if (gpu == 0)
            {
                var name = new StringBuilder(128);
                Native.Hr(Native.gpu_create((uint)o.Adapter, out gpu, name, 128, out var luid));
                evidence.Write("adapter", new { index = o.Adapter, name = name.ToString(), luid = $"{luid:X16}" });
            }
            // gpu-active holds the same amount in low/high phases; only activity changes.
            int mib = level == 0 ? 0 : Scaled(Is("gpu-active") || level == 1 ? 128 : 512);
            mib = (mib + 3) / 4 * 4;
            Native.Hr(Native.gpu_resize(gpu, (uint)mib));
        }
        retainedLevel = level;
        lastTick = time.Elapsed.TotalSeconds;
    }
    public void Tick(Phase phase)
    {
        if (workerFailure != null) throw new InvalidOperationException("CPU worker failed", workerFailure);
        double now = time.Elapsed.TotalSeconds;
        // Cap catch-up work; measured throughput, not requested throughput, is authoritative.
        double elapsed = Math.Clamp(now - lastTick, 0, 0.2);
        lastTick = now;
        int bytes = (int)(targetRate * elapsed);
        if ((Is("io-read") || Is("io-write")) && bytes > 0)
        {
            while (bytes > 0)
            {
                if (io!.Position >= io.Length) io.Position = 0;
                int amount = (int)Math.Min(Math.Min(bytes, ioBuffer.Length), io.Length - io.Position);
                if (Is("io-read")) { amount = io.Read(ioBuffer, 0, amount); if (amount == 0) throw new IOException("Unexpected EOF"); }
                else io.Write(ioBuffer, 0, amount);
                transferred += amount; bytes -= amount;
            }
        }
        if (IsDotnet && targetRate > 0)
        {
            while (bytes >= 16 * 1024) { AllocateShort(); bytes -= 16 * 1024; allocated += 16 * 1024; }
        }
        if (IsGpu && phase.Level > 0 && now - lastGpuTick >= (Is("gpu-active") || o.Mode == "screenshot" ? 0.1 : 1))
        {
            uint passes = Is("gpu-active") || o.Mode == "screenshot" ? (phase.Level == 1 ? 1u : 4u) : 1u;
            Native.Hr(Native.gpu_tick(gpu, passes));
            gpuPasses += passes; lastGpuTick = time.Elapsed.TotalSeconds;
        }
    }
    [MethodImpl(MethodImplOptions.NoInlining)] private static void AllocateShort()
    {
        var bytes = new byte[16 * 1024]; bytes[0] = 1; allocationSink = bytes;
    }
    private void Collect(string reason)
    {
        evidence.Write("gc-request", new { reason });
        GC.Collect(2, GCCollectionMode.Forced, true, false);
        GC.WaitForPendingFinalizers();
        GC.Collect(2, GCCollectionMode.Forced, true, false);
    }
    private static void SetManaged(List<byte[]> list, int mib, int chunk, bool pinned)
    {
        Resize(list, mib * MiB / chunk, () => { var a = GC.AllocateArray<byte>(chunk, pinned); for (int i = 0; i < a.Length; i += 4096) a[i] = 1; return a; }, _ => { });
    }
    private unsafe void SetMemory(int stage)
    {
        int chunk = Scaled(256) * MiB;
        int count = stage == 0 ? 0 : stage == 3 ? 3 : 1;
        Resize(memory, count, () => Native.Require(Native.VirtualAlloc(0, (nuint)chunk, 0x3000, 0x04)),
            p => Native.Require(Native.VirtualFree(p, 0, 0x8000)));
        if (stage >= 2)
            foreach (nint region in memory)
                for (int i = 0; i < chunk; i += 4096) ((byte*)region)[i] = 1;
        touched = stage >= 2;
    }
    private void StartThreads(int count, bool cpu)
    {
        stop.Reset();
        for (int i = 0; i < count; ++i)
        {
            var t = new Thread(() => {
                try
                {
                    if (!cpu) { stop.WaitOne(); return; }
                    while (!stop.WaitOne(0))
                    {
                        var cycle = Stopwatch.StartNew();
                        int duty = cpuDuty;
                        long iterations = 0;
                        while (cycle.Elapsed.TotalMilliseconds < duty) { Thread.SpinWait(100); ++iterations; }
                        Interlocked.Add(ref cpuIterations, iterations);
                        stop.WaitOne(Math.Max(1, 100 - (int)cycle.ElapsedMilliseconds));
                    }
                }
                catch (Exception ex) { workerFailure = ex; }
            }) { IsBackground = true, Name = cpu ? "workload-cpu" : "workload-idle" };
            threads.Add(t); t.Start();
        }
    }
    private void StopThreads()
    {
        stop.Set();
        foreach (var t in threads) if (!t.Join(3000)) throw new TimeoutException("Workload thread did not stop");
        threads.Clear(); stop.Reset();
    }
    private void EnsureScratch()
    {
        if (scratch != null) return;
        string candidate = Path.Combine(o.Output, $"scratch-{Guid.NewGuid():N}");
        Directory.CreateDirectory(candidate); scratch = candidate;
    }
    private static void Resize<T>(List<T> list, int count, Func<T> create, Action<T> release)
    {
        while (list.Count > count) { release(list[^1]); list.RemoveAt(list.Count - 1); }
        while (list.Count < count) list.Add(create());
    }
    public object State()
    {
        object? gpuState = null;
        if (gpu != 0)
        {
            Native.Hr(Native.gpu_stats(gpu, out var local, out var nonlocal, out var budget, out var mib));
            gpuState = new { resource_payload_bytes = (long)mib * MiB, local_usage_bytes = local,
                nonlocal_usage_bytes = nonlocal, local_budget_bytes = budget, completed_passes = gpuPasses };
        }
        return new { cpu_workers = Is("cpu") ? threads.Count : 0, cpu_duty_percent = cpuDuty,
            cpu_iterations = Interlocked.Read(ref cpuIterations), native_committed_bytes = (long)memory.Count * Scaled(256) * MiB,
            memory_touched = touched, additional_threads = threads.Count, event_handles = handles.Count,
            held_files = files.Select(f => Path.GetFileName(f.Name)).ToArray(), user_windows = windows.Count, gdi_brushes = brushes.Count,
            target_bytes_per_second = targetRate, transferred_bytes_total = transferred, short_allocated_payload_bytes_total = allocated,
            gen2_candidate_payload_bytes = gen2.Sum(a => (long)a.Length), loh_payload_bytes = loh.Sum(a => (long)a.Length),
            poh_payload_bytes = poh.Sum(a => (long)a.Length), gpu = gpuState };
    }
    public void Dispose()
    {
        if (disposed) return;
        disposed = true;
        var failures = new List<Exception>();
        void Clean(Action action) { try { action(); } catch (Exception ex) { failures.Add(ex); } }
        Clean(StopThreads);
        Clean(() => stop.Dispose());
        foreach (var h in handles) Clean(h.Dispose);
        foreach (var f in files) Clean(f.Dispose);
        if (io != null) Clean(io.Dispose);
        foreach (var h in windows) Clean(() => Native.Require(Native.DestroyWindow(h)));
        foreach (var h in brushes) Clean(() => Native.Require(Native.DeleteObject(h)));
        foreach (var p in memory) Clean(() => Native.Require(Native.VirtualFree(p, 0, 0x8000)));
        gen2.Clear(); loh.Clear(); poh.Clear(); allocationSink = null;
        if (gpu != 0) Clean(() => { Native.gpu_destroy(gpu); gpu = 0; });
        // Only this process's newly created scratch directory is removed, never the output root.
        if (scratch != null) Clean(() => {
            string outputRoot = Path.GetFullPath(o.Output).TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
            string scratchPath = Path.GetFullPath(scratch);
            if (!scratchPath.StartsWith(outputRoot, StringComparison.OrdinalIgnoreCase))
                throw new IOException("Scratch directory escaped the output root");
            Directory.Delete(scratchPath, true);
        });
        if (failures.Count > 0) throw new AggregateException("Workload cleanup failed", failures);
    }
}
