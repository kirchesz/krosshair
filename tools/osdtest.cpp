// RTSS OSD positioning test bench.
// Opens a topmost 640x480 D3D11 window (RTSS hooks it), then for each variant from the variants file
// writes hypertext into two private OSD slots, waits, captures the client area from the screen and
// reports the bounding box of everything that is not the clear color near the window center.
//
// Variants file: one per line, "label|slot1 text|slot2 text". "\b" in the text becomes a backspace.

#include <windows.h>
#include <d3d11.h>
#include <stdio.h>
#include <string>
#include <vector>
#include <fstream>
#include "RTSSSharedMemory.h"

#pragma comment(lib, "d3d11.lib")
#pragma comment(lib, "user32.lib")
#pragma comment(lib, "gdi32.lib")

static const int W = 640, H = 480;
static const float CLEAR[4] = { 30 / 255.f, 30 / 255.f, 30 / 255.f, 1.f };

static IDXGISwapChain* g_swap;
static ID3D11Device* g_dev;
static ID3D11DeviceContext* g_ctx;
static ID3D11RenderTargetView* g_rtv;

static void Frame()
{
	g_ctx->OMSetRenderTargets(1, &g_rtv, NULL);
	g_ctx->ClearRenderTargetView(g_rtv, CLEAR);
	g_swap->Present(1, 0);
}

static void PumpFor(DWORD ms)
{
	DWORD t0 = GetTickCount();
	while (GetTickCount() - t0 < ms)
	{
		MSG msg;
		while (PeekMessage(&msg, NULL, 0, 0, PM_REMOVE)) { TranslateMessage(&msg); DispatchMessage(&msg); }
		Frame();
	}
}

static std::string Unescape(const std::string& s)
{
	std::string r;
	for (size_t i = 0; i < s.size(); i++)
	{
		if (s[i] == '\\' && i + 1 < s.size() && s[i + 1] == 'b') { r += '\b'; i++; }
		else r += s[i];
	}
	return r;
}

static void WriteSlot(LPCSTR owner, const std::string& text)
{
	HANDLE h = OpenFileMappingA(FILE_MAP_ALL_ACCESS, FALSE, "RTSSSharedMemoryV2");
	if (!h) return;
	LPRTSS_SHARED_MEMORY p = (LPRTSS_SHARED_MEMORY)MapViewOfFile(h, FILE_MAP_ALL_ACCESS, 0, 0, 0);
	if (p && p->dwSignature == 'RTSS')
	{
		for (DWORD pass = 0; pass < 2; pass++)
		{
			bool done = false;
			for (DWORD i = 1; i < p->dwOSDArrSize; i++)
			{
				RTSS_SHARED_MEMORY::LPRTSS_SHARED_MEMORY_OSD_ENTRY e = (RTSS_SHARED_MEMORY::LPRTSS_SHARED_MEMORY_OSD_ENTRY)((LPBYTE)p + p->dwOSDArrOffset + i * p->dwOSDEntrySize);
				if (pass && !e->szOSDOwner[0]) lstrcpyA(e->szOSDOwner, owner);
				if (!lstrcmpA(e->szOSDOwner, owner))
				{
					DWORD t0 = GetTickCount();
					bool locked = false;
					while (!(locked = !_interlockedbittestandset(&p->dwBusy, 0)) && GetTickCount() - t0 < 500) Sleep(1);
					lstrcpynA(e->szOSDEx, text.c_str(), sizeof(e->szOSDEx) - 1);
					if (locked) p->dwBusy = 0;
					p->dwOSDFrame++;
					done = true;
					break;
				}
			}
			if (done) break;
		}
	}
	if (p) UnmapViewOfFile(p);
	CloseHandle(h);
}

static void ReleaseSlot(LPCSTR owner)
{
	HANDLE h = OpenFileMappingA(FILE_MAP_ALL_ACCESS, FALSE, "RTSSSharedMemoryV2");
	if (!h) return;
	LPRTSS_SHARED_MEMORY p = (LPRTSS_SHARED_MEMORY)MapViewOfFile(h, FILE_MAP_ALL_ACCESS, 0, 0, 0);
	if (p && p->dwSignature == 'RTSS')
	{
		for (DWORD i = 1; i < p->dwOSDArrSize; i++)
		{
			RTSS_SHARED_MEMORY::LPRTSS_SHARED_MEMORY_OSD_ENTRY e = (RTSS_SHARED_MEMORY::LPRTSS_SHARED_MEMORY_OSD_ENTRY)((LPBYTE)p + p->dwOSDArrOffset + i * p->dwOSDEntrySize);
			if (!lstrcmpA(e->szOSDOwner, owner)) { SecureZeroMemory(e, p->dwOSDEntrySize); p->dwOSDFrame++; }
		}
	}
	if (p) UnmapViewOfFile(p);
	CloseHandle(h);
}

// Captures the client area and returns bbox of non-background pixels within +-R of the center.
static void Measure(HWND hWnd, const char* label, const char* savePath)
{
	POINT org = { 0, 0 };
	ClientToScreen(hWnd, &org);
	HDC scr = GetDC(NULL);
	HDC mem = CreateCompatibleDC(scr);
	BITMAPINFO bi = { 0 };
	bi.bmiHeader.biSize = sizeof(bi.bmiHeader);
	bi.bmiHeader.biWidth = W; bi.bmiHeader.biHeight = -H;
	bi.bmiHeader.biPlanes = 1; bi.bmiHeader.biBitCount = 32;
	BYTE* px = NULL;
	HBITMAP bmp = CreateDIBSection(scr, &bi, DIB_RGB_COLORS, (void**)&px, NULL, 0);
	HGDIOBJ old = SelectObject(mem, bmp);
	BitBlt(mem, 0, 0, W, H, scr, org.x, org.y, SRCCOPY | CAPTUREBLT);
	SelectObject(mem, old);

	const int R = 200;
	int minx = 99999, miny = 99999, maxx = -1, maxy = -1;
	for (int y = H / 2 - R; y < H / 2 + R; y++)
		for (int x = W / 2 - R; x < W / 2 + R; x++)
		{
			BYTE* c = px + (y * W + x) * 4;
			if (abs(c[0] - 30) + abs(c[1] - 30) + abs(c[2] - 30) > 40)
			{
				if (x < minx) minx = x; if (x > maxx) maxx = x;
				if (y < miny) miny = y; if (y > maxy) maxy = y;
			}
		}

	if (maxx < 0)
		printf("%-48s  nothing found near center\n", label);
	else
		printf("%-48s  bbox x=%d..%d y=%d..%d  center offset dx=%+.1f dy=%+.1f\n", label, minx, maxx, miny, maxy,
			(minx + maxx) / 2.0 - (W - 1) / 2.0, (miny + maxy) / 2.0 - (H - 1) / 2.0);

	fflush(stdout);

	if (savePath)
	{
		BITMAPFILEHEADER fh = { 0 };
		fh.bfType = 'MB';
		fh.bfOffBits = sizeof(fh) + sizeof(bi.bmiHeader);
		fh.bfSize = fh.bfOffBits + W * H * 4;
		FILE* f = NULL;
		if (!fopen_s(&f, savePath, "wb") && f)
		{
			fwrite(&fh, sizeof(fh), 1, f);
			fwrite(&bi.bmiHeader, sizeof(bi.bmiHeader), 1, f);
			fwrite(px, W * H * 4, 1, f);
			fclose(f);
		}
	}

	DeleteObject(bmp);
	DeleteDC(mem);
	ReleaseDC(NULL, scr);
}

int main(int argc, char** argv)
{
	if (argc < 2) { printf("usage: osdtest variants.txt [snapshot_dir]\n"); return 1; }
	SetProcessDPIAware();

	std::vector<std::string> lines;
	if (strcmp(argv[1], "--watch"))
	{
		std::ifstream in(argv[1]);
		std::string l;
		while (std::getline(in, l)) if (!l.empty() && l[0] != '#') lines.push_back(l);
	}

	WNDCLASSA wc = { 0 };
	wc.lpfnWndProc = DefWindowProcA;
	wc.hInstance = GetModuleHandle(NULL);
	wc.lpszClassName = "RTSSOSDTest";
	wc.hCursor = LoadCursor(NULL, IDC_ARROW);
	RegisterClassA(&wc);

	RECT rc = { 0, 0, W, H };
	DWORD style = WS_POPUP | WS_VISIBLE;
	int sx = (GetSystemMetrics(SM_CXSCREEN) - W) / 2, sy = (GetSystemMetrics(SM_CYSCREEN) - H) / 2;
	HWND hWnd = CreateWindowExA(WS_EX_TOPMOST | WS_EX_TOOLWINDOW, "RTSSOSDTest", "RTSS OSD test", style, sx, sy, W, H, NULL, NULL, wc.hInstance, NULL);

	DXGI_SWAP_CHAIN_DESC sd = { 0 };
	sd.BufferCount = 1;
	sd.BufferDesc.Width = W; sd.BufferDesc.Height = H;
	sd.BufferDesc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
	sd.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
	sd.OutputWindow = hWnd;
	sd.SampleDesc.Count = 1;
	sd.Windowed = TRUE;
	sd.SwapEffect = DXGI_SWAP_EFFECT_DISCARD;
	if (FAILED(D3D11CreateDeviceAndSwapChain(NULL, D3D_DRIVER_TYPE_HARDWARE, NULL, 0, NULL, 0, D3D11_SDK_VERSION, &sd, &g_swap, &g_dev, NULL, &g_ctx)))
	{
		printf("D3D11 init failed\n"); return 1;
	}
	ID3D11Texture2D* bb = NULL;
	g_swap->GetBuffer(0, __uuidof(ID3D11Texture2D), (void**)&bb);
	g_dev->CreateRenderTargetView(bb, NULL, &g_rtv);
	bb->Release();

	// Let RTSS hook the process and start rendering OSD
	PumpFor(2500);

	if (!strcmp(argv[1], "--watch"))
	{
		// Measure-only mode: someone else (RTSS_Crosshair) writes the OSD.
		PumpFor(1000);
		Measure(hWnd, "watch", argc > 2 ? argv[2] : NULL);
		DestroyWindow(hWnd);
		return 0;
	}

	int n = 0;
	for (const std::string& l : lines)
	{
		size_t a = l.find('|'), b = (a == std::string::npos) ? std::string::npos : l.find('|', a + 1);
		if (b == std::string::npos) continue;
		std::string label = l.substr(0, a);
		WriteSlot("OSDTest_1", Unescape(l.substr(a + 1, b - a - 1)));
		WriteSlot("OSDTest_2", Unescape(l.substr(b + 1)));
		PumpFor(1200);
		char path[MAX_PATH] = "";
		if (argc > 2) sprintf_s(path, "%s\\v%02d.bmp", argv[2], n);
		Measure(hWnd, label.c_str(), argc > 2 ? path : NULL);
		n++;
	}

	ReleaseSlot("OSDTest_1");
	ReleaseSlot("OSDTest_2");
	PumpFor(100);
	DestroyWindow(hWnd);
	return 0;
}
