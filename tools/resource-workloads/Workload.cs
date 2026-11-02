using System.Diagnostics;
using System.Runtime.CompilerServices;

// Only managed allocation workloads live here. Native demos use native.cpp.
internal sealed class Workload : IDisposable
{
    private const int MiB = 1024 * 1024;
    private readonly Options o;
    private readonly Evidence evidence;
    private readonly List<byte[]> gen2 = [], loh = [], poh = [];
    private readonly Stopwatch time = Stopwatch.StartNew();
    private int retainedLevel = -1, targetRate;
    private double lastTick;
    private long allocated;
    private static volatile object? allocationSink;
    public Workload(Options options, Evidence evidence) { o = options; this.evidence = evidence; }
    private int Scaled(int value) => Math.Max(1, (int)(value * o.Scale));
    private bool Is(string name) => o.Workload == name;
    public void Apply(Phase phase)
    {
        int level = o.Mode == "screenshot" && phase.Level > 0 ? (phase.MemoryStage == 3 ? 2 : 1) : phase.Level;
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
        retainedLevel = level;
        lastTick = time.Elapsed.TotalSeconds;
    }
    public void Tick(Phase phase)
    {
        double now = time.Elapsed.TotalSeconds;
        int bytes = (int)(targetRate * Math.Clamp(now - lastTick, 0, 0.2));
        lastTick = now;
        while (bytes >= 16 * 1024) { AllocateShort(); bytes -= 16 * 1024; allocated += 16 * 1024; }
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
    private static void Resize<T>(List<T> list, int count, Func<T> create, Action<T> release)
    {
        while (list.Count > count) { release(list[^1]); list.RemoveAt(list.Count - 1); }
        while (list.Count < count) list.Add(create());
    }
    public object State() => new {
        target_bytes_per_second = targetRate, short_allocated_payload_bytes_total = allocated,
        gen2_candidate_payload_bytes = gen2.Sum(a => (long)a.Length), loh_payload_bytes = loh.Sum(a => (long)a.Length),
        poh_payload_bytes = poh.Sum(a => (long)a.Length)
    };
    public void Dispose() { gen2.Clear(); loh.Clear(); poh.Clear(); allocationSink = null; }
}
