// Native resource workload and process-tree launcher (Windows x64).
#define NOMINMAX
#include <windows.h>
#include <algorithm>
#include <atomic>
#include <chrono>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <map>
#include <psapi.h>
#include <sstream>
#include <stdexcept>
#include <string>
#include <thread>
#include <tlhelp32.h>
#include <vector>
namespace fs = std::filesystem;
using Clock = std::chrono::steady_clock;
using U64 = unsigned long long;
constexpr int MiB = 1024 * 1024;
static std::atomic<bool> cancelled{false};
BOOL WINAPI control(DWORD event) {
    if (event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT) {
        cancelled = true;
        return TRUE;
    }
    return FALSE;
}
void require(bool ok, const char *operation) {
    if (!ok)
        throw std::runtime_error(std::string(operation) + " failed: " + std::to_string(GetLastError()));
}
void hr(HRESULT result) {
    if (FAILED(result))
        throw std::runtime_error("GPU HRESULT " + std::to_string(result));
}
std::string utf8(const std::wstring &s) {
    if (s.empty())
        return {};
    int n = WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, s.data(), static_cast<int>(s.size()), nullptr,
                                0, nullptr, nullptr);
    require(n > 0, "UTF-8 conversion");
    std::string r(n, '\0');
    require(WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, s.data(), static_cast<int>(s.size()), r.data(),
                                n, nullptr, nullptr) == n,
            "UTF-8 conversion");
    return r;
}
// Small JSON encoder: values are encoded before composing objects/arrays.
std::string quote(const std::string &s) {
    std::ostringstream out;
    out << '"';
    for (unsigned char c : s) {
        if (c == '"' || c == '\\')
            out << '\\' << c;
        else if (c < 32)
            out << "\\u" << std::hex << std::setw(4) << std::setfill('0') << static_cast<int>(c);
        else
            out << c;
    }
    out << '"';
    return out.str();
}
std::string q(const std::wstring &s) { return quote(utf8(s)); }
template <class T> std::string num(T v) {
    std::ostringstream s;
    s.imbue(std::locale::classic());
    s << std::setprecision(15) << v;
    return s.str();
}
std::string object(std::initializer_list<std::pair<std::string, std::string>> fields) {
    std::string r = "{";
    for (const auto &f : fields) {
        if (r.size() > 1)
            r += ',';
        r += quote(f.first) + ':' + f.second;
    }
    return r + '}';
}
std::string array(const std::vector<std::string> &values) {
    std::string r = "[";
    for (const auto &v : values) {
        if (r.size() > 1)
            r += ',';
        r += v;
    }
    return r + ']';
}
std::string timestamp(FILETIME ft) {
    SYSTEMTIME t{};
    require(FileTimeToSystemTime(&ft, &t) != 0, "timestamp");
    char b[40];
    sprintf_s(b, "%04u-%02u-%02uT%02u:%02u:%02u.%03uZ", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute,
              t.wSecond, t.wMilliseconds);
    return quote(b);
}
std::string utc() {
    FILETIME t;
    GetSystemTimeAsFileTime(&t);
    return timestamp(t);
}
double elapsed(Clock::time_point start) {
    return std::chrono::duration<double>(Clock::now() - start).count();
}
struct Handle {
    HANDLE h = nullptr;
    Handle() = default;
    explicit Handle(HANDLE v) : h(v) { require(v && v != INVALID_HANDLE_VALUE, "handle creation"); }
    ~Handle() {
        if (h && h != INVALID_HANDLE_VALUE)
            CloseHandle(h);
    }
    Handle(const Handle &) = delete;
    Handle &operator=(const Handle &) = delete;
    Handle(Handle &&other) noexcept : h(other.h) { other.h = nullptr; }
    Handle &operator=(Handle &&other) noexcept {
        if (this != &other) {
            if (h)
                CloseHandle(h);
            h = other.h;
            other.h = nullptr;
        }
        return *this;
    }
};
fs::path executable() {
    std::wstring b(32768, L'\0');
    DWORD n = GetModuleFileNameW(nullptr, b.data(), static_cast<DWORD>(b.size()));
    require(n && n < b.size(), "executable path");
    b.resize(n);
    return b;
}
const std::vector<std::wstring> nativeNames = {L"cpu",        L"memory",  L"threads",  L"handles",
                                               L"files",      L"io-read", L"io-write", L"gpu",
                                               L"gpu-active", L"objects", L"user",     L"gdi"};
const std::vector<std::wstring> managedNames = {L"dotnet",     L"dotnet-short", L"dotnet-gen2",
                                                L"dotnet-loh", L"dotnet-poh",   L"dotnet-frag"};
bool contains(const std::vector<std::wstring> &names, const std::wstring &name) {
    return std::find(names.begin(), names.end(), name) != names.end();
}
struct Options {
    std::wstring workload, mode = L"verify";
    double scale = 1, timeScale = 1;
    int seconds = 120, adapter = 0, workers = 1;
    bool forceGc = false;
    fs::path output, stop;
    std::vector<std::wstring> workloads;
    bool launcher = false, worker = false;
};
double number(const std::wstring &s) {
    size_t used = 0;
    double v = std::stod(s, &used);
    if (used != s.size() || !std::isfinite(v))
        throw std::runtime_error("Invalid number");
    return v;
}
int integer(const std::wstring &s) {
    double v = number(s);
    if (v != std::floor(v) || v < 0 || v > 3600)
        throw std::runtime_error("Invalid integer");
    return static_cast<int>(v);
}
Options parse(int argc, wchar_t **argv) {
    Options o;
    auto stem = executable().stem().wstring();
    o.worker = stem == L"demo-worker";
    o.launcher = o.worker || stem == L"demo-main";
    o.workload = o.launcher ? (o.worker ? L"worker" : L"main") : stem.substr(5);
    std::map<std::wstring, std::wstring> values;
    const std::vector<std::wstring> allowed = {
        L"--workload", L"--workloads", L"--mode",   L"--scale",     L"--time-scale", L"--seconds",
        L"--adapter",  L"--force-gc",  L"--output", L"--stop-file", L"--cpu-workers"};
    for (int i = 1; i < argc; i += 2) {
        if (i + 1 >= argc || !contains(allowed, argv[i]) || !values.emplace(argv[i], argv[i + 1]).second)
            throw std::runtime_error("Unknown, duplicate or incomplete option");
    }
    auto get = [&](const wchar_t *key, std::wstring fallback) {
        auto it = values.find(key);
        return it == values.end() ? fallback : it->second;
    };
    if (o.launcher && values.count(L"--workload"))
        throw std::runtime_error("Launcher requires --workloads");
    if (!o.launcher && values.count(L"--workloads"))
        throw std::runtime_error("Workload requires --workload");
    o.workload = get(L"--workload", o.workload);
    o.mode = get(L"--mode", L"verify");
    o.scale = number(get(L"--scale", L"1"));
    o.timeScale = number(get(L"--time-scale", L"1"));
    o.seconds = integer(get(L"--seconds", o.mode == L"verify" ? L"120" : L"600"));
    o.adapter = integer(get(L"--adapter", L"0"));
    o.workers = integer(get(L"--cpu-workers", L"1"));
    auto gc = get(L"--force-gc", L"false");
    if (gc != L"true" && gc != L"false")
        throw std::runtime_error("Invalid force-gc");
    o.forceGc = gc == L"true";
    if ((o.mode != L"verify" && o.mode != L"screenshot") || o.scale < 0.0625 || o.scale > 1 ||
        o.timeScale < 0.025 || o.timeScale > 1 || o.seconds < 1 || o.seconds > 3600 || o.adapter > 15 ||
        o.workers < 1 ||
        o.workers > static_cast<int>(std::min(16u, std::max(1u, std::thread::hardware_concurrency()))))
        throw std::runtime_error("Invalid bounded option");
    if (!o.launcher && !contains(nativeNames, o.workload))
        throw std::runtime_error(
            "Native executable requires a native workload; use demo-dotnet.exe for .NET");
    o.output =
        fs::absolute(get(L"--output", L"logs/workloads/run-" + std::to_wstring(GetCurrentProcessId())));
    auto stop = get(L"--stop-file", L"");
    if (!stop.empty())
        o.stop = fs::absolute(stop);
    else if (o.launcher)
        o.stop = o.output / L"stop.request";
    if (o.launcher) {
        auto names = get(L"--workloads", L"cpu,memory,dotnet,gpu,io-write,objects");
        std::wistringstream s(names);
        std::wstring name;
        while (std::getline(s, name, L',')) {
            if ((!contains(nativeNames, name) && !contains(managedNames, name)) ||
                contains(o.workloads, name))
                throw std::runtime_error("Invalid or duplicate workload");
            o.workloads.push_back(name);
        }
        if (o.workloads.empty() || names.back() == L',')
            throw std::runtime_error("Empty workload");
    }
    return o;
}
struct Phase {
    std::string name;
    int level, stage;
    bool operator!=(const Phase &p) const { return name != p.name || level != p.level || stage != p.stage; }
};
Phase phaseAt(const Options &o, double seconds) {
    double t = seconds / o.timeScale;
    if (o.mode == L"screenshot") {
        if (t < 15)
            return {"baseline", 0, 0};
        if (o.workload == L"memory") {
            // Thirty-second plateaus make committed-memory steps visible at normal sampling speed.
            // Stage 4 is the touched two-chunk plateau between stages 2 (one) and 3 (three).
            constexpr int stages[] = {2, 4, 3, 4, 2, 0};
            constexpr int levels[] = {1, 2, 3, 2, 1, 0};
            int step = static_cast<int>((t - 15) / 30) % 6;
            return {t < 135 ? "warmup" : "capture", levels[step], stages[step]};
        }
        int cycle = static_cast<int>((t - 15) / 10) % 4;
        return {t < 135 ? "warmup" : "capture", cycle == 0 || cycle == 3 ? 1 : 2, t < 45 ? 2 : 3};
    }
    if (o.workload == L"memory") {
        if (t < 15)
            return {"baseline", 0, 0};
        if (t < 35)
            return {"commit", 1, 1};
        if (t < 55)
            return {"touch", 1, 2};
        if (t < 75)
            return {"grow", 2, 3};
        if (t < 95)
            return {"partial-release", 1, 2};
        return {"released", 0, 0};
    }
    if (t < 15)
        return {"baseline", 0, 0};
    if (t < 45)
        return {"low", 1, 2};
    if (t < 75)
        return {"high", 2, 3};
    if (t < 105)
        return {"released", 0, 0};
    return {"settled", 0, 0};
}
struct Evidence {
    Handle file;
    Options o;
    Clock::time_point start = Clock::now();
    explicit Evidence(const Options &options) : o(options) {
        fs::create_directories(o.output);
        if (o.launcher) {
            // Persistent reservation prevents mixing process manifests across runs.
            auto owner = o.output / (o.worker ? L"worker.owner" : L"main.owner");
            Handle reservation(CreateFileW(owner.c_str(), GENERIC_WRITE, FILE_SHARE_READ, nullptr, CREATE_NEW,
                                           FILE_ATTRIBUTE_NORMAL, nullptr));
        }
        auto path = o.output / (o.workload + L"-" + std::to_wstring(GetCurrentProcessId()) + L".jsonl");
        file = Handle(CreateFileW(path.c_str(), GENERIC_WRITE, FILE_SHARE_READ, nullptr, CREATE_NEW,
                                  FILE_ATTRIBUTE_NORMAL, nullptr));
    }
    void write(const std::string &kind, const std::string &data) {
        auto line = object({{"schema", "1"},
                            {"kind", quote(kind)},
                            {"utc", utc()},
                            {"elapsed_seconds", num(elapsed(start))},
                            {"pid", num(GetCurrentProcessId())},
                            {"workload", q(o.workload)},
                            {"data", data}}) +
                    "\n";
        DWORD n = 0;
        require(WriteFile(file.h, line.data(), static_cast<DWORD>(line.size()), &n, nullptr) &&
                    n == line.size(),
                "evidence write");
    }
};
std::string observed() {
    PROCESS_MEMORY_COUNTERS_EX memory{};
    memory.cb = sizeof(memory);
    require(GetProcessMemoryInfo(GetCurrentProcess(), reinterpret_cast<PROCESS_MEMORY_COUNTERS *>(&memory),
                                 sizeof(memory)) != 0,
            "memory counters");
    DWORD handles = 0;
    require(GetProcessHandleCount(GetCurrentProcess(), &handles) != 0, "handle count");
    FILETIME c, e, k, u;
    require(GetProcessTimes(GetCurrentProcess(), &c, &e, &k, &u) != 0, "process times");
    auto ticks = [](FILETIME t) { return (static_cast<U64>(t.dwHighDateTime) << 32) | t.dwLowDateTime; };
    Handle snapshot(CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0));
    THREADENTRY32 entry{};
    entry.dwSize = sizeof(entry);
    int threads = 0;
    if (Thread32First(snapshot.h, &entry))
        do {
            if (entry.th32OwnerProcessID == GetCurrentProcessId())
                ++threads;
        } while (Thread32Next(snapshot.h, &entry));
    return object({{"private_bytes", num(memory.PrivateUsage)},
                   {"working_set_bytes", num(memory.WorkingSetSize)},
                   {"threads", num(threads)},
                   {"handles", num(handles)},
                   {"cpu_seconds", num((ticks(k) + ticks(u)) / 1e7)},
                   {"user_objects", num(GetGuiResources(GetCurrentProcess(), GR_USEROBJECTS))},
                   {"gdi_objects", num(GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS))}});
}
struct Gpu;
extern "C" HRESULT gpu_create(unsigned, Gpu **, wchar_t *, unsigned, U64 *) noexcept;
extern "C" HRESULT gpu_resize(Gpu *, unsigned) noexcept;
extern "C" HRESULT gpu_tick(Gpu *, unsigned) noexcept;
extern "C" HRESULT gpu_stats(Gpu *, U64 *, U64 *, U64 *, unsigned *) noexcept;
extern "C" void gpu_destroy(Gpu *) noexcept;
template <class T, class Make, class Free>
void resize(std::vector<T> &list, size_t count, Make make, Free release) {
    while (list.size() > count) {
        release(list.back());
        list.pop_back();
    }
    while (list.size() < count)
        list.push_back(make());
}
struct Workload {
    const Options &o;
    Evidence &evidence;
    std::vector<void *> memory;
    std::vector<HWND> windows;
    std::vector<HBRUSH> brushes;
    std::vector<Handle> handles, files;
    std::vector<std::thread> threads;
    std::atomic<bool> stopThreads{false};
    std::atomic<int> duty{0};
    std::atomic<U64> iterations{0};
    Handle io;
    fs::path scratch;
    std::vector<char> buffer = std::vector<char>(MiB);
    Gpu *gpu = nullptr;
    bool touched = false, cleaned = false;
    int retained = -1, rate = 0;
    U64 transferred = 0, passes = 0;
    double last = 0, lastGpu = -1;
    Clock::time_point start = Clock::now();
    Workload(const Options &options, Evidence &e) : o(options), evidence(e) {}
    ~Workload() {
        if (!cleaned)
            try {
                cleanup();
            } catch (...) {
            }
    }
    bool is(const wchar_t *name) const { return o.workload == name; }
    bool isGpu() const { return is(L"gpu") || is(L"gpu-active"); }
    int scaled(int value) const { return std::max(1, static_cast<int>(value * o.scale)); }
    void stopWorkers() {
        stopThreads = true;
        for (auto &t : threads)
            t.join();
        threads.clear();
        stopThreads = false;
    }
    void startWorkers(int count, bool cpu) {
        for (int i = 0; i < count; ++i)
            threads.emplace_back([this, cpu]() {
                while (!stopThreads) {
                    if (!cpu) {
                        Sleep(10);
                        continue;
                    }
                    auto cycle = Clock::now();
                    U64 n = 0;
                    int work = duty.load();
                    while (!stopThreads && elapsed(cycle) * 1000 < work) {
                        for (int j = 0; j < 100; ++j)
                            YieldProcessor();
                        ++n;
                    }
                    iterations += n;
                    while (!stopThreads && elapsed(cycle) < 0.1)
                        Sleep(1);
                }
            });
    }
    void ensureScratch() {
        if (!scratch.empty())
            return;
        auto path = o.output / (L"scratch-" + std::to_wstring(GetCurrentProcessId()));
        if (!fs::create_directory(path))
            throw std::runtime_error("Scratch directory already exists");
        scratch = path;
    }
    void apply(Phase p) {
        int level = o.mode == L"screenshot" && p.level > 0 ? (p.stage == 3 ? 2 : 1) : p.level;
        if (is(L"cpu")) {
            if (threads.empty())
                startWorkers(o.workers, true);
            duty = p.level == 0 ? 0 : p.level == 1 ? 50 : 100;
        }
        if (is(L"memory")) {
            int chunk = scaled(256) * MiB;
            resize(
                memory,
                p.stage == 0   ? 0
                : p.stage == 3 ? 3
                : p.stage == 4 ? 2
                               : 1,
                [&]() {
                    auto v = VirtualAlloc(nullptr, chunk, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
                    require(v != nullptr, "VirtualAlloc");
                    return v;
                },
                [](void *v) { require(VirtualFree(v, 0, MEM_RELEASE) != 0, "VirtualFree"); });
            if (p.stage >= 2)
                for (auto v : memory)
                    for (int i = 0; i < chunk; i += 4096)
                        static_cast<volatile char *>(v)[i] = 1;
            touched = p.stage >= 2;
        }
        if (is(L"threads") && level != retained) {
            stopWorkers();
            startWorkers(level == 0 ? 0 : scaled(level == 1 ? 8 : 32), false);
        }
        if (is(L"handles"))
            resize(
                handles, level == 0 ? 0 : scaled(level == 1 ? 200 : 1000),
                []() { return Handle(CreateEventW(nullptr, TRUE, FALSE, nullptr)); }, [](Handle &) {});
        if (is(L"files")) {
            ensureScratch();
            resize(
                files, level == 0 ? 0 : scaled(level == 1 ? 4 : 16),
                [&]() {
                    auto path = scratch / (L"held-" + std::to_wstring(files.size()) + L".dat");
                    return Handle(CreateFileW(path.c_str(), GENERIC_READ | GENERIC_WRITE,
                                              FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr,
                                              OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr));
                },
                [](Handle &) {});
        }
        if (is(L"io-read") || is(L"io-write")) {
            ensureScratch();
            if (!io.h) {
                auto path = scratch / L"transfer.dat";
                io = Handle(CreateFileW(path.c_str(), GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ, nullptr,
                                        CREATE_NEW, FILE_ATTRIBUTE_NORMAL, nullptr));
                DWORD n;
                for (int i = 0; i < 64; ++i)
                    require(WriteFile(io.h, buffer.data(), MiB, &n, nullptr) && n == MiB, "prepare file");
                require(FlushFileBuffers(io.h) != 0, "flush file");
                LARGE_INTEGER zero{};
                require(SetFilePointerEx(io.h, zero, nullptr, FILE_BEGIN) != 0, "rewind");
                evidence.write("setup", object({{"file_bytes", num(64 * MiB)},
                                                {"preparation_write_bytes", num(64 * MiB)}}));
            }
            rate = p.level == 0 ? 0 : scaled(p.level == 1 ? 8 * MiB : 32 * MiB);
        }
        if (is(L"objects") || is(L"user"))
            resize(
                windows, level == 0 ? 0 : scaled(level == 1 ? 10 : 50),
                []() {
                    HWND h = CreateWindowExW(0, L"STATIC", L"Resource workload", 0, 0, 0, 1, 1, nullptr,
                                             nullptr, nullptr, nullptr);
                    require(h != nullptr, "CreateWindow");
                    return h;
                },
                [](HWND h) { require(DestroyWindow(h) != 0, "DestroyWindow"); });
        if (is(L"objects") || is(L"gdi"))
            resize(
                brushes, level == 0 ? 0 : scaled(level == 1 ? 100 : 500),
                []() {
                    HBRUSH h = CreateSolidBrush(0x806040);
                    require(h != nullptr, "CreateSolidBrush");
                    return h;
                },
                [](HBRUSH h) { require(DeleteObject(h) != 0, "DeleteObject"); });
        if (isGpu()) {
            if (!gpu) {
                wchar_t name[128]{};
                U64 luid;
                hr(gpu_create(o.adapter, &gpu, name, 128, &luid));
                std::ostringstream id;
                id << std::uppercase << std::hex << std::setw(16) << std::setfill('0') << luid;
                evidence.write(
                    "adapter",
                    object({{"index", num(o.adapter)}, {"name", q(name)}, {"luid", quote(id.str())}}));
            }
            int mib = level == 0 ? 0 : scaled(is(L"gpu-active") || level == 1 ? 128 : 512);
            hr(gpu_resize(gpu, static_cast<unsigned>((mib + 3) / 4 * 4)));
        }
        retained = level;
        last = elapsed(start);
    }
    void tick(Phase p) {
        double now = elapsed(start);
        int bytes = static_cast<int>(rate * std::clamp(now - last, 0.0, 0.2));
        last = now;
        while (io.h && bytes > 0) {
            LARGE_INTEGER zero{}, position{};
            require(SetFilePointerEx(io.h, zero, &position, FILE_CURRENT) != 0, "file position");
            if (position.QuadPart >= 64LL * MiB) {
                require(SetFilePointerEx(io.h, zero, &position, FILE_BEGIN) != 0, "rewind");
            }
            DWORD amount = static_cast<DWORD>(
                      std::min<long long>(std::min(bytes, MiB), 64LL * MiB - position.QuadPart)),
                  done = 0;
            require((is(L"io-read") ? ReadFile(io.h, buffer.data(), amount, &done, nullptr)
                                    : WriteFile(io.h, buffer.data(), amount, &done, nullptr)) &&
                        done > 0,
                    "transfer");
            transferred += done;
            bytes -= static_cast<int>(done);
        }
        if (isGpu() && p.level > 0 &&
            now - lastGpu >= (is(L"gpu-active") || o.mode == L"screenshot" ? 0.1 : 1)) {
            unsigned n = is(L"gpu-active") || o.mode == L"screenshot" ? (p.level == 1 ? 1u : 4u) : 1u;
            hr(gpu_tick(gpu, n));
            passes += n;
            lastGpu = elapsed(start);
        }
    }
    std::string state() {
        std::string g = "null";
        if (gpu) {
            U64 local, nonlocal, budget;
            unsigned mib;
            hr(gpu_stats(gpu, &local, &nonlocal, &budget, &mib));
            g = object({{"resource_payload_bytes", num(static_cast<U64>(mib) * MiB)},
                        {"local_usage_bytes", num(local)},
                        {"nonlocal_usage_bytes", num(nonlocal)},
                        {"local_budget_bytes", num(budget)},
                        {"completed_passes", num(passes)}});
        }
        std::vector<std::string> names;
        for (size_t i = 0; i < files.size(); ++i)
            names.push_back(quote("held-" + num(i) + ".dat"));
        return object({{"cpu_workers", num(is(L"cpu") ? threads.size() : 0)},
                       {"cpu_duty_percent", num(duty.load())},
                       {"cpu_iterations", num(iterations.load())},
                       {"native_committed_bytes", num(memory.size() * scaled(256) * MiB)},
                       {"memory_touched", touched ? "true" : "false"},
                       {"additional_threads", num(threads.size())},
                       {"event_handles", num(handles.size())},
                       {"held_files", array(names)},
                       {"user_windows", num(windows.size())},
                       {"gdi_brushes", num(brushes.size())},
                       {"target_bytes_per_second", num(rate)},
                       {"transferred_bytes_total", num(transferred)},
                       {"gpu", g}});
    }
    void cleanup() {
        stopWorkers();
        handles.clear();
        files.clear();
        io = Handle();
        bool ok = true;
        for (auto h : windows)
            ok = (DestroyWindow(h) != 0) && ok;
        windows.clear();
        for (auto h : brushes)
            ok = (DeleteObject(h) != 0) && ok;
        brushes.clear();
        for (auto p : memory)
            ok = (VirtualFree(p, 0, MEM_RELEASE) != 0) && ok;
        memory.clear();
        if (gpu) {
            gpu_destroy(gpu);
            gpu = nullptr;
        }
        if (!scratch.empty()) { // Only files created by this workload are removed; never recursively delete a
                                // supplied path.
            for (auto &entry : fs::directory_iterator(scratch))
                if (entry.is_regular_file())
                    fs::remove(entry.path());
            fs::remove(scratch);
            scratch.clear();
        }
        cleaned = true;
        require(ok, "resource cleanup");
    }
};
bool stopping(const Options &o) { return cancelled || (!o.stop.empty() && fs::exists(o.stop)); }
void requestStop(const Options &o) {
    if (!o.stop.empty() && !fs::exists(o.stop)) {
        std::ofstream f(o.stop);
        f << "stop";
        if (!f)
            throw std::runtime_error("Cannot write stop request");
    }
}
std::wstring argQuote(const std::wstring &s) {
    std::wstring r = L"\"";
    size_t slashes = 0;
    for (wchar_t c : s) {
        if (c == L'\\') {
            ++slashes;
            continue;
        }
        if (c == L'"')
            r.append(slashes * 2 + 1, L'\\');
        else
            r.append(slashes, L'\\');
        slashes = 0;
        r += c;
    }
    r.append(slashes * 2, L'\\');
    return r + L'"';
}
std::wstring wideNum(double value) {
    auto s = num(value);
    return std::wstring(s.begin(), s.end());
}
std::vector<std::wstring> childArgs(const Options &o, const std::wstring &name,
                                    const std::vector<std::wstring> &group) {
    std::vector<std::wstring> args;
    if (group.empty())
        args = {L"--workload", name};
    else {
        std::wstring list;
        for (auto &n : group) {
            if (!list.empty())
                list += L',';
            list += n;
        }
        args = {L"--workloads", list};
    }
    std::vector<std::wstring> common = {
        L"--mode",        o.mode,
        L"--scale",       wideNum(o.scale),
        L"--time-scale",  wideNum(o.timeScale),
        L"--seconds",     std::to_wstring(o.seconds),
        L"--adapter",     std::to_wstring(o.adapter),
        L"--cpu-workers", std::to_wstring(o.workers),
        L"--force-gc",    o.forceGc ? L"true" : L"false",
        L"--output",      (group.empty() ? o.output / name : o.output).wstring(),
        L"--stop-file",   o.stop.wstring()};
    args.insert(args.end(), common.begin(), common.end());
    return args;
}
struct Child {
    Handle process;
    DWORD pid;
    std::wstring name, workload;
};
void writeFile(const fs::path &path, const std::string &text) {
    auto temporary = path;
    temporary += L".tmp-" + std::to_wstring(GetCurrentProcessId());
    {
        std::ofstream f(temporary, std::ios::binary);
        f << text << '\n';
        f.flush();
        if (!f)
            throw std::runtime_error("Manifest write failed");
    }
    require(MoveFileExW(temporary.c_str(), path.c_str(), MOVEFILE_REPLACE_EXISTING) != 0, "publish manifest");
}

int launch(const Options &o, Evidence &e) {
    std::vector<Child> children;
    std::vector<std::wstring> grouped;
    int result = 0;
    auto manifest = [&]() {
        std::vector<std::string> rows;
        for (auto &c : children)
            rows.push_back(object(
                {{"workload", q(c.workload)},
                 {"pid", num(c.pid)},
                 {"parent_pid", num(GetCurrentProcessId())},
                 {"name", q(c.name)},
                 {"evidence", q((c.workload == L"worker" ? o.output : o.output / c.workload).wstring())}}));
        writeFile(o.output / (o.worker ? L"worker-processes.json" : L"processes.json"), array(rows));
    };
    auto spawn = [&](const std::wstring &name, const std::vector<std::wstring> &group) {
        auto alias = name;
        if (contains(managedNames, name))
            alias = L"dotnet";
        else if (name == L"gpu-active")
            alias = L"gpu";
        else if (name == L"user" || name == L"gdi")
            alias = L"objects";
        auto exe = executable().parent_path() / (L"demo-" + alias + L".exe");
        std::wstring command = argQuote(exe.wstring());
        for (auto &a : childArgs(o, name, group))
            command += L" " + argQuote(a);
        STARTUPINFOW si{};
        si.cb = sizeof(si);
        PROCESS_INFORMATION pi{};
        require(CreateProcessW(exe.c_str(), command.data(), nullptr, nullptr, FALSE, CREATE_NO_WINDOW,
                               nullptr, exe.parent_path().c_str(), &si, &pi) != 0,
                "CreateProcess");
        Handle thread(pi.hThread);
        children.push_back({Handle(pi.hProcess), pi.dwProcessId, exe.filename().wstring(), name});
        manifest();
        auto ready = o.output / (o.worker ? L"worker.ready" : L"main.ready");
        if (!fs::exists(ready))
            writeFile(ready, num(GetCurrentProcessId()));
        e.write("child", object({{"pid", num(pi.dwProcessId)},
                                 {"name", q(exe.filename().wstring())},
                                 {"parent_pid", num(GetCurrentProcessId())}}));
    };
    auto wait = [&](Child &child) {
        while (WaitForSingleObject(child.process.h, 50) == WAIT_TIMEOUT) {
            if (stopping(o))
                requestStop(o);
        }
        DWORD code;
        require(GetExitCodeProcess(child.process.h, &code) != 0, "child exit");
        if (code != 0)
            result = 1;
    };
    try {
        for (auto &name : o.workloads) {
            if (stopping(o))
                break;
            if (!o.worker && (contains(managedNames, name) || name == L"io-read" || name == L"io-write" ||
                              name == L"objects" || name == L"user" || name == L"gdi")) {
                grouped.push_back(name);
                continue;
            }
            spawn(name, {});
            if (o.mode == L"verify") {
                wait(children.back());
                if (result)
                    break;
            }
        }
        if (!result && !grouped.empty() && !stopping(o))
            spawn(L"worker", grouped);
        // The readiness marker only means processes have been launched, never that the run succeeded.
        auto ready = o.output / (o.worker ? L"worker.ready" : L"main.ready");
        if (!fs::exists(ready))
            writeFile(ready, num(GetCurrentProcessId()));
        bool running = true;
        while (running) {
            running = false;
            for (auto &child : children) {
                DWORD code;
                require(GetExitCodeProcess(child.process.h, &code) != 0, "child status");
                if (code == STILL_ACTIVE)
                    running = true;
                else if (code != 0)
                    result = 1;
            }
            if (result || stopping(o))
                requestStop(o);
            if (running)
                Sleep(50);
        }
    } catch (const std::exception &error) {
        result = 1;
        try {
            requestStop(o);
        } catch (...) { /* Child duration limits still apply. */
        }
        for (auto &c : children) {
            // Do not let a broken evidence/stop path orphan already started children.
            require(WaitForSingleObject(c.process.h, INFINITE) == WAIT_OBJECT_0, "child cleanup");
        }
        e.write("error", object({{"message", quote(error.what())}}));
    }
    e.write("cleanup", "{\"success\":true}");
    e.write("end", object({{"success", result == 0 ? "true" : "false"},
                           {"reason", quote(result        ? "failure"
                                            : stopping(o) ? "stop-request"
                                                          : "duration")}}));
    return result;
}
int wmain(int argc, wchar_t **argv) {
    if (argc == 2 && std::wstring(argv[1]) == L"--help") {
        std::cout << "Native Windows x64 demos. demo-main/demo-worker: --workloads comma-separated "
                     "names.\nWorkload executables: --workload NAME (defaults to executable name).\n--mode "
                     "verify|screenshot --scale 0.0625..1 --time-scale 0.025..1 --seconds 1..3600\n--adapter "
                     "0..15 --cpu-workers 1..min(16,logical CPUs) --output DIR --stop-file PATH\nOnly "
                     "demo-dotnet.exe uses .NET. Ctrl+C or stop file requests cleanup.\n";
        return 0;
    }
    Options o;
    try {
        o = parse(argc, argv);
    } catch (const std::exception &error) {
        std::cerr << error.what() << '\n';
        return 2;
    }
    SetConsoleCtrlHandler(control, TRUE);
    try {
        Evidence e(o);
        FILETIME c, exit, k, u;
        require(GetProcessTimes(GetCurrentProcess(), &c, &exit, &k, &u) != 0, "start time");
        e.write("start", object({{"process_start_utc", timestamp(c)},
                                 {"runtime", quote("native")},
                                 {"logical_cpus", num(std::thread::hardware_concurrency())},
                                 {"options", object({{"Workload", q(o.workload)},
                                                     {"Mode", q(o.mode)},
                                                     {"Scale", num(o.scale)},
                                                     {"TimeScale", num(o.timeScale)},
                                                     {"Seconds", num(o.seconds)},
                                                     {"Adapter", num(o.adapter)},
                                                     {"CpuWorkers", num(o.workers)}})}}));
        if (o.launcher)
            return launch(o, e);
        Workload work(o, e);
        int result = 0;
        std::string reason = "duration";
        try {
            auto clock = Clock::now();
            double next = 0;
            Phase previous{"", -1, -1};
            while (elapsed(clock) < o.seconds * o.timeScale) {
                if (stopping(o)) {
                    reason = "stop-request";
                    break;
                }
                auto phase = phaseAt(o, elapsed(clock));
                if (phase != previous) {
                    e.write("phase-request", object({{"Name", quote(phase.name)},
                                                     {"Level", num(phase.level)},
                                                     {"MemoryStage", num(phase.stage)}}));
                    work.apply(phase);
                    e.write("phase", object({{"Name", quote(phase.name)},
                                             {"Level", num(phase.level)},
                                             {"MemoryStage", num(phase.stage)},
                                             {"actual", work.state()}}));
                    previous = phase;
                }
                work.tick(phase);
                if (elapsed(clock) >= next) {
                    e.write("sample", object({{"phase", quote(phase.name)},
                                              {"actual", work.state()},
                                              {"observed", observed()}}));
                    next = elapsed(clock) + std::max(0.1, o.timeScale);
                }
                Sleep(50);
            }
        } catch (const std::exception &error) {
            result = 1;
            reason = "failure";
            e.write("error", object({{"message", quote(error.what())}}));
            std::cerr << error.what() << '\n';
        }
        try {
            work.cleanup();
            e.write("cleanup", "{\"success\":true}");
        } catch (const std::exception &error) {
            result = 1;
            reason = "cleanup-failure";
            e.write("cleanup", object({{"success", "false"}, {"message", quote(error.what())}}));
        }
        e.write("end", object({{"success", result == 0 ? "true" : "false"}, {"reason", quote(reason)}}));
        return result;
    } catch (const std::exception &error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
