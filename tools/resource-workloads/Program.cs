using System.Diagnostics;
using System.Globalization;
using System.Runtime.InteropServices;
using System.Text.Json;

internal sealed record Options(string Workload, string Mode, double Scale, double TimeScale,
    int Seconds, int Adapter, bool ForceGc, string Output, string? StopFile, int CpuWorkers)
{
    public static readonly string[] Workloads = ["cpu", "memory", "threads", "handles", "files", "io-read", "io-write",
        "dotnet", "dotnet-short", "dotnet-gen2", "dotnet-loh", "dotnet-poh", "dotnet-frag", "gpu", "gpu-active", "objects", "user", "gdi"];
    public static Options Parse(string[] args)
    {
        var values = new Dictionary<string, string>();
        string[] allowed = ["workload", "mode", "scale", "time-scale", "seconds", "adapter", "force-gc", "output", "stop-file", "cpu-workers"];
        for (int i = 0; i < args.Length; i += 2)
        {
            if (i + 1 >= args.Length || !args[i].StartsWith("--") || !allowed.Contains(args[i][2..]) ||
                !values.TryAdd(args[i][2..], args[i + 1])) throw new ArgumentException("Unknown, duplicate or incomplete option.");
        }
        string Get(string key, string fallback) => values.GetValueOrDefault(key, fallback);
        double Number(string key, string fallback) => double.Parse(Get(key, fallback), CultureInfo.InvariantCulture);
        string workload = Get("workload", "cpu"), mode = Get("mode", "verify");
        double scale = Number("scale", "1"), timeScale = Number("time-scale", "1");
        int seconds = int.Parse(Get("seconds", mode == "verify" ? "120" : "600"), CultureInfo.InvariantCulture);
        int workers = int.Parse(Get("cpu-workers", "1"), CultureInfo.InvariantCulture);
        int adapter = int.Parse(Get("adapter", "0"), CultureInfo.InvariantCulture);
        if (!Workloads.Contains(workload) || (mode != "verify" && mode != "screenshot") ||
            !double.IsFinite(scale) || scale < 0.0625 || scale > 1 ||
            !double.IsFinite(timeScale) || timeScale < 0.025 || timeScale > 1 ||
            seconds < 1 || seconds > 3600 || adapter < 0 || adapter > 15 || workers < 1 || workers > Math.Min(16, Environment.ProcessorCount))
            throw new ArgumentException("Invalid workload/mode or bounded numeric option.");
        return new(workload, mode, scale, timeScale, seconds, adapter, bool.Parse(Get("force-gc", "false")),
            Get("output", Path.Combine("logs", "workloads", $"{DateTime.UtcNow:yyyyMMdd-HHmmss}-{Environment.ProcessId}")),
            values.GetValueOrDefault("stop-file"), workers);
    }
}

internal readonly record struct Phase(string Name, int Level, int MemoryStage);
internal static class Schedule
{
    public static Phase At(Options o, double seconds)
    {
        double t = seconds / o.TimeScale;
        if (o.Mode == "screenshot")
        {
            if (t < 15) return new("baseline", 0, 0);
            int level = (int)((t - 15) / 10) % 4 is 0 or 3 ? 1 : 2;
            // Retained resources stay at the high level once the warmup has completed.
            return new(t < 135 ? "warmup" : "capture", level, t < 45 ? 2 : 3);
        }
        if (o.Workload == "memory") return t switch {
            < 15 => new("baseline", 0, 0), < 35 => new("commit", 1, 1),
            < 55 => new("touch", 1, 2), < 75 => new("grow", 2, 3),
            < 95 => new("partial-release", 1, 2), _ => new("released", 0, 0) };
        return t switch { < 15 => new("baseline", 0, 0), < 45 => new("low", 1, 2),
            < 75 => new("high", 2, 3), < 105 => new("released", 0, 0), _ => new("settled", 0, 0) };
    }
}

internal sealed class Evidence : IDisposable
{
    private readonly StreamWriter writer;
    private readonly Stopwatch clock = Stopwatch.StartNew();
    private readonly Options options;
    public Evidence(Options options)
    {
        this.options = options;
        Directory.CreateDirectory(options.Output);
        writer = new StreamWriter(new FileStream(Path.Combine(options.Output, $"{options.Workload}-{Environment.ProcessId}.jsonl"),
            FileMode.CreateNew, FileAccess.Write, FileShare.Read)) { AutoFlush = true };
    }
    public void Write(string kind, object data) => writer.WriteLine(JsonSerializer.Serialize(new {
        schema = 1, kind, utc = DateTimeOffset.UtcNow, elapsed_seconds = clock.Elapsed.TotalSeconds,
        pid = Environment.ProcessId, workload = options.Workload, data
    }));
    public void Dispose() => writer.Dispose();
}

internal static class Program
{
    public static int Main(string[] args)
    {
        if (args.Contains("--help"))
        {
            Console.WriteLine("Resource workloads (Windows x64, .NET 10)\n" +
                "--workload " + string.Join('|', Options.Workloads) + "\n" +
                "--mode verify|screenshot --scale 0.0625..1 --time-scale 0.025..1\n" +
                "--seconds 1..3600 (scenario seconds; multiplied by time-scale) --adapter 0..15\n" +
                "--force-gc true|false --cpu-workers 1..min(16,logical CPUs) --output DIRECTORY --stop-file PATH\n" +
                "Ctrl+C or the stop file requests cleanup. Default: CPU verify, 120 seconds.\n" +
                "All executables are managed; native-resource modes also have .NET runtime overhead.");
            return 0;
        }
        if (args.SequenceEqual(new[] { "--self-test" })) return SelfTest();
        Options o;
        try { o = Options.Parse(args); }
        catch (Exception ex) { Console.Error.WriteLine(ex.Message); return 2; }
        if (!OperatingSystem.IsWindows() || !Environment.Is64BitProcess) return 2;
        using var cancel = new CancellationTokenSource();
        ConsoleCancelEventHandler handler = (_, e) => { e.Cancel = true; cancel.Cancel(); };
        Console.CancelKeyPress += handler;
        try
        {
            using var evidence = new Evidence(o);
            evidence.Write("start", new { options = o, logical_cpus = Environment.ProcessorCount,
                process_start_utc = Process.GetCurrentProcess().StartTime.ToUniversalTime(), runtime = Environment.Version.ToString() });
            Console.WriteLine($"{o.Workload}: PID {Environment.ProcessId}, output {Path.GetFullPath(o.Output)}");
            Workload? workload = null;
            int result = 0;
            string reason = "duration";
            try
            {
                workload = new Workload(o, evidence);
                var clock = Stopwatch.StartNew();
                double nextSample = 0;
                Phase? previous = null;
                while (clock.Elapsed.TotalSeconds < o.Seconds * o.TimeScale)
                {
                    if (cancel.IsCancellationRequested || (o.StopFile != null && File.Exists(o.StopFile))) { reason = "stop-request"; break; }
                    var phase = Schedule.At(o, clock.Elapsed.TotalSeconds);
                    if (phase != previous)
                    {
                        evidence.Write("phase-request", new { phase.Name, phase.Level, phase.MemoryStage });
                        workload.Apply(phase);
                        evidence.Write("phase", new { phase.Name, phase.Level, phase.MemoryStage, actual = workload.State() });
                        previous = phase;
                    }
                    workload.Tick(phase);
                    if (clock.Elapsed.TotalSeconds >= nextSample)
                    {
                        evidence.Write("sample", new { phase = phase.Name, actual = workload.State(), observed = Observe() });
                        nextSample = clock.Elapsed.TotalSeconds + Math.Max(0.1, o.TimeScale);
                    }
                    cancel.Token.WaitHandle.WaitOne(50);
                }
            }
            catch (Exception ex)
            {
                result = 1; reason = "failure";
                evidence.Write("error", new { type = ex.GetType().Name, message = ex.Message, hresult = ex.HResult });
                Console.Error.WriteLine($"{o.Workload}: {ex.Message}");
            }
            finally
            {
                try { workload?.Dispose(); evidence.Write("cleanup", new { success = true }); }
                catch (Exception ex) { result = 1; reason = "cleanup-failure"; evidence.Write("cleanup", new { success = false, message = ex.Message }); }
            }
            evidence.Write("end", new { success = result == 0, reason });
            return result;
        }
        catch (Exception ex) { Console.Error.WriteLine(ex); return 1; }
        finally { Console.CancelKeyPress -= handler; }
    }
    private static object Observe()
    {
        using var p = Process.GetCurrentProcess();
        var gc = GC.GetGCMemoryInfo();
        return new { private_bytes = p.PrivateMemorySize64, working_set_bytes = p.WorkingSet64,
            threads = p.Threads.Count, handles = p.HandleCount, cpu_seconds = p.TotalProcessorTime.TotalSeconds,
            user_objects = Native.GetGuiResources(p.Handle, 1), gdi_objects = Native.GetGuiResources(p.Handle, 0),
            allocated_bytes_total = GC.GetTotalAllocatedBytes(), heap_bytes_after_gc = gc.HeapSizeBytes,
            gc_committed_bytes = gc.TotalCommittedBytes, fragmentation_bytes_after_gc = gc.FragmentedBytes,
            gc_counts = new[] { GC.CollectionCount(0), GC.CollectionCount(1), GC.CollectionCount(2) },
            generations_after_gc = gc.GenerationInfo.ToArray().Select(g => new { bytes = g.SizeAfterBytes, fragmented = g.FragmentationAfterBytes }).ToArray() };
    }
    private static int SelfTest()
    {
        void Check(bool condition) { if (!condition) throw new Exception("Self-test assertion failed"); }
        var o = Options.Parse([]);
        Check(Schedule.At(o, 14.99).Level == 0);
        Check(Schedule.At(o, 15).Level == 1);
        Check(Schedule.At(o, 45).Level == 2);
        Check(Schedule.At(o, 75).Level == 0);
        var memory = o with { Workload = "memory" };
        Check(Schedule.At(memory, 20).MemoryStage == 1);
        Check(Schedule.At(memory, 40).MemoryStage == 2);
        Check(Schedule.At(memory, 60).MemoryStage == 3);
        Check(Schedule.At(memory, 80).MemoryStage == 2);
        Check(Schedule.At(memory, 100).MemoryStage == 0);
        var photo = o with { Mode = "screenshot" };
        Check(Schedule.At(photo, 150).Name == "capture");
        Check(Schedule.At(photo, 150).MemoryStage == 3);
        foreach (string[] invalid in new[] { new[] { "--scale", "NaN" }, new[] { "--scale", "2" },
            new[] { "--seconds", "0" }, new[] { "--adapter", "-1" }, new[] { "--mode", "bad" },
            new[] { "--workload", "bad" }, new[] { "--scale" }, new[] { "--scale", "1", "--scale", "1" } })
        {
            bool rejected = false;
            try { Options.Parse(invalid); } catch { rejected = true; }
            Check(rejected);
        }
        Console.WriteLine("Schedule boundaries and input limits passed.");
        return 0;
    }
}
