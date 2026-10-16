// Hardware-only D3D11 workload. No rendering window or software fallback.
#include <windows.h>
#include <d3d11.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <vector>
#include <memory>
#include <new>
using Microsoft::WRL::ComPtr;
struct Gpu {
    ComPtr<IDXGIAdapter3> adapter;
    ComPtr<ID3D11Device> device;
    ComPtr<ID3D11DeviceContext> context;
    std::vector<ComPtr<ID3D11Texture2D>> textures;
    std::vector<ComPtr<ID3D11RenderTargetView>> views;
    unsigned long long passes = 0;
};
#define API extern "C" __declspec(dllexport)
API HRESULT gpu_create(unsigned index, Gpu** result, wchar_t* name, unsigned capacity,
    unsigned long long* luid) noexcept {
    *result = nullptr;
    try {
        ComPtr<IDXGIFactory1> factory;
        HRESULT hr = CreateDXGIFactory1(IID_PPV_ARGS(&factory));
        if (FAILED(hr)) return hr;
        ComPtr<IDXGIAdapter1> selected;
        unsigned hardware = 0;
        for (unsigned i = 0;; ++i) {
            ComPtr<IDXGIAdapter1> candidate;
            hr = factory->EnumAdapters1(i, &candidate);
            if (FAILED(hr)) return hr;
            DXGI_ADAPTER_DESC1 desc{};
            if (FAILED(hr = candidate->GetDesc1(&desc))) return hr;
            if (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) continue;
            if (hardware++ != index) continue;
            selected = candidate;
            wcsncpy_s(name, capacity, desc.Description, _TRUNCATE);
            *luid = (static_cast<unsigned long long>(static_cast<unsigned>(desc.AdapterLuid.HighPart)) << 32)
                | desc.AdapterLuid.LowPart;
            break;
        }
        auto gpu = std::make_unique<Gpu>();
        if (FAILED(hr = selected.As(&gpu->adapter))) return hr;
        if (FAILED(hr = D3D11CreateDevice(selected.Get(), D3D_DRIVER_TYPE_UNKNOWN, nullptr, 0,
            nullptr, 0, D3D11_SDK_VERSION, &gpu->device, nullptr, &gpu->context))) return hr;
        *result = gpu.release();
        return S_OK;
    } catch (...) { return E_OUTOFMEMORY; }
}
API HRESULT gpu_resize(Gpu* gpu, unsigned mib) noexcept {
    // Each RGBA8 texture is 1024 x 1024 = 4 MiB logical payload.
    if (!gpu || mib > 1024 || mib % 4) return E_INVALIDARG;
    try {
        const size_t count = mib / 4;
        while (gpu->views.size() > count) {
            gpu->views.pop_back();
            gpu->textures.pop_back();
        }
        while (gpu->views.size() < count) {
            D3D11_TEXTURE2D_DESC desc{};
            desc.Width = desc.Height = 1024;
            desc.MipLevels = desc.ArraySize = 1;
            desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
            desc.SampleDesc.Count = 1;
            desc.Usage = D3D11_USAGE_DEFAULT;
            desc.BindFlags = D3D11_BIND_RENDER_TARGET;
            ComPtr<ID3D11Texture2D> texture;
            HRESULT hr = gpu->device->CreateTexture2D(&desc, nullptr, &texture);
            if (FAILED(hr)) return hr;
            ComPtr<ID3D11RenderTargetView> view;
            hr = gpu->device->CreateRenderTargetView(texture.Get(), nullptr, &view);
            if (FAILED(hr)) return hr;
            const float color[4] = {0.2f, 0.4f, 0.6f, 1.0f};
            gpu->context->ClearRenderTargetView(view.Get(), color);
            gpu->textures.push_back(texture);
            gpu->views.push_back(view);
        }
        gpu->context->Flush();
        return gpu->device->GetDeviceRemovedReason();
    } catch (...) { return E_OUTOFMEMORY; }
}
API HRESULT gpu_tick(Gpu* gpu, unsigned passes) noexcept {
    if (!gpu || passes > 16) return E_INVALIDARG;
    for (unsigned i = 0; i < passes; ++i) {
        const float color[4] = {static_cast<float>((gpu->passes % 100) / 100.0), 0.4f, 0.6f, 1.0f};
        for (auto& view : gpu->views) gpu->context->ClearRenderTargetView(view.Get(), color);
        ++gpu->passes;
    }
    gpu->context->Flush();
    // Bound outstanding GPU work. Do not report submitted work as completed work.
    D3D11_QUERY_DESC desc{D3D11_QUERY_EVENT, 0};
    ComPtr<ID3D11Query> query;
    HRESULT hr = gpu->device->CreateQuery(&desc, &query);
    if (FAILED(hr)) return hr;
    gpu->context->End(query.Get());
    gpu->context->Flush();
    const ULONGLONG deadline = GetTickCount64() + 2000;
    while ((hr = gpu->context->GetData(query.Get(), nullptr, 0, 0)) == S_FALSE) {
        if (GetTickCount64() >= deadline) return HRESULT_FROM_WIN32(WAIT_TIMEOUT);
        Sleep(1);
    }
    return hr;
}
API HRESULT gpu_stats(Gpu* gpu, unsigned long long* local, unsigned long long* nonlocal,
    unsigned long long* budget, unsigned* mib) noexcept {
    DXGI_QUERY_VIDEO_MEMORY_INFO a{}, b{};
    HRESULT hr = gpu->adapter->QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &a);
    if (FAILED(hr)) return hr;
    hr = gpu->adapter->QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, &b);
    if (FAILED(hr)) return hr;
    *local = a.CurrentUsage; *nonlocal = b.CurrentUsage; *budget = a.Budget;
    *mib = static_cast<unsigned>(gpu->views.size() * 4);
    return S_OK;
}
API void gpu_destroy(Gpu* gpu) noexcept { delete gpu; }
