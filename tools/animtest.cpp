// RTSS sprite animation test bench.
//
// Question it answers: if a client rewrites the OSD text N times a second to show the next frame
// of a sprite sheet, what reaches the screen? Frames skipped, shown twice, blank moments, and how
// even the timing is.
//
// It opens a 640x480 D3D11 window (RTSS hooks it like a game), and a writer thread cycles through
// the frames of an atlas at the given rate. Frame i of the atlas is a bar (i + 1) * 4 pixels wide,
// drawn at the top center of the window, so the frame on screen can be read back from the width of
// the bar in a screen capture.
//
// usage: animtest <atlas.png> <frames> <rate hz> <seconds> [fps cap, 0 = none]
//        animtest <atlas.png> <frames> <rate hz> <seconds> <fps cap> nocap
//          Measures only the frame rate: no screen capture, which costs far more than RTSS does.
//          Rate 0 writes the first frame once and leaves it.
//        animtest --watch <frames> <frame ms> <seconds>
//        animtest --texts <file> <rate hz> <seconds>
//          Frame rate only, cycling through the lines of the file as the OSD text ("\b" is a
//          backspace). A line starting with "static|" is written once into a slot before them.
//          Writes nothing: watches what another program (krosshair playing a GIF made like the
//          atlas, one frame per bar width, at the top center anchor) puts on screen.
// Build: cl /EHsc /I "<RTSS folder>\SDK\Include" animtest.cpp

#include <windows.h>
#include <d3d11.h>
#include <stdio.h>
#include <stdlib.h>
#include <math.h>
#include <string>
#include <vector>
#include "RTSSSharedMemory.h"

#pragma comment(lib, "d3d11.lib")
#pragma comment(lib, "user32.lib")
#pragma comment(lib, "gdi32.lib")
#pragma comment(lib, "winmm.lib")

static const int W = 640, H = 480;
static const float CLEAR[4] = { 30 / 255.f, 30 / 255.f, 30 / 255.f, 1.f };
static const int SPRITE_W = 64, SPRITE_H = 8, PITCH = 10, STEP = 4;

static IDXGISwapChain* g_swap;
static ID3D11Device* g_dev;
static ID3D11DeviceContext* g_ctx;
static ID3D11RenderTargetView* g_rtv;

static LARGE_INTEGER g_freq;
static double Now()
{
	LARGE_INTEGER t; QueryPerformanceCounter(&t);
	return (double)t.QuadPart * 1000.0 / (double)g_freq.QuadPart;
}

static void Frame()
{
	MSG msg;
	while (PeekMessage(&msg, NULL, 0, 0, PM_REMOVE)) { TranslateMessage(&msg); DispatchMessage(&msg); }
	g_ctx->OMSetRenderTargets(1, &g_rtv, NULL);
	g_ctx->ClearRenderTargetView(g_rtv, CLEAR);
	g_swap->Present(0, 0);
}

// Writes the text into the slot of `owner`. Returns how long it waited for the OSD lock, in ms,
// or -1 if it gave up (the text is then not written, like krosshair does).
static double WriteSlot(LPRTSS_SHARED_MEMORY p, LPCSTR owner, const std::string& text)
{
	for (DWORD pass = 0; pass < 2; pass++)
		for (DWORD i = 1; i < p->dwOSDArrSize; i++)
		{
			RTSS_SHARED_MEMORY::LPRTSS_SHARED_MEMORY_OSD_ENTRY e = (RTSS_SHARED_MEMORY::LPRTSS_SHARED_MEMORY_OSD_ENTRY)((LPBYTE)p + p->dwOSDArrOffset + i * p->dwOSDEntrySize);
			if (pass && !e->szOSDOwner[0]) lstrcpyA(e->szOSDOwner, owner);
			if (lstrcmpA(e->szOSDOwner, owner)) continue;
			double t0 = Now();
			while (_interlockedbittestandset(&p->dwBusy, 0))
			{
				if (Now() - t0 > 20) return -1;
				YieldProcessor();
			}
			double waited = Now() - t0;
			lstrcpynA(e->szOSDEx, text.c_str(), sizeof(e->szOSDEx) - 1);
			p->dwBusy = 0;
			p->dwOSDFrame++;
			return waited;
		}
	return -1;
}

static void ReleaseSlot(LPRTSS_SHARED_MEMORY p, LPCSTR owner)
{
	for (DWORD i = 1; i < p->dwOSDArrSize; i++)
	{
		RTSS_SHARED_MEMORY::LPRTSS_SHARED_MEMORY_OSD_ENTRY e = (RTSS_SHARED_MEMORY::LPRTSS_SHARED_MEMORY_OSD_ENTRY)((LPBYTE)p + p->dwOSDArrOffset + i * p->dwOSDEntrySize);
		if (!lstrcmpA(e->szOSDOwner, owner)) { SecureZeroMemory(e, p->dwOSDEntrySize); p->dwOSDFrame++; }
	}
}

struct Writer
{
	LPRTSS_SHARED_MEMORY mem;
	std::string atlas;
	int frames;
	double periodMs;
	volatile LONG stop;
	volatile LONG current;      // frame index written last
	int writes, gaveUp;
	double maxWait, sumWait;
};

// --texts mode: the texts to cycle through, and one written once into a slot before them.
static std::vector<std::string> g_texts;
static std::string g_static;

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

static std::string TextFor(const std::string& atlas, int frame)
{
	if (!g_texts.empty()) return g_texts[frame % g_texts.size()];
	char buf[512];
	sprintf_s(buf, "<LI=\"%s\"><P1><L241><S=10><I=%d,%d,0,%d,%d,%d>\b<P=0,0><L254>", atlas.c_str(), SPRITE_W, SPRITE_H, frame * PITCH, SPRITE_W, SPRITE_H);
	return buf;
}

static DWORD WINAPI WriterThread(LPVOID param)
{
	Writer* w = (Writer*)param;
	double next = Now();
	int frame = 0;
	while (!w->stop)
	{
		double wait = WriteSlot(w->mem, "AnimTest_1", TextFor(w->atlas, frame));
		if (wait < 0) w->gaveUp++;
		else
		{
			w->writes++;
			w->sumWait += wait;
			if (wait > w->maxWait) w->maxWait = wait;
			InterlockedExchange(&w->current, frame);
		}
		frame = (frame + 1) % w->frames;
		next += w->periodMs;
		// Sleep most of the way, then spin: the period has to be exact.
		while (!w->stop)
		{
			double left = next - Now();
			if (left <= 0) break;
			if (left > 2) Sleep(1); else YieldProcessor();
		}
	}
	return 0;
}

// Reads the frame on screen from the width of the bar at the top center. -1: nothing there.
static int Observe(HDC scr, HDC mem, BYTE* px, POINT org)
{
	const int x0 = W / 2 - SPRITE_W / 2 - 4, cw = SPRITE_W + 8, ch = SPRITE_H + 4;
	BitBlt(mem, 0, 0, cw, ch, scr, org.x + x0, org.y, SRCCOPY);
	int minx = 9999, maxx = -1;
	for (int y = 0; y < ch; y++)
		for (int x = 0; x < cw; x++)
		{
			BYTE* c = px + (y * cw + x) * 4;
			if (c[1] > 180 && c[0] < 100 && c[2] < 100)      // lime
			{
				if (x < minx) minx = x;
				if (x > maxx) maxx = x;
			}
		}
	if (maxx < 0) return -1;
	return (maxx - minx + 1 + STEP / 2) / STEP - 1;
}

int main(int argc, char** argv)
{
	if (argc < 5) { printf("usage: animtest <atlas.png> <frames> <rate hz> <seconds> [fps cap]\n"); return 1; }
	std::string atlas = argv[1];
	bool watch = atlas == "--watch";
	if (atlas == "--texts")
	{
		FILE* tf = NULL;
		fopen_s(&tf, argv[2], "rb");
		char line[4096];
		while (tf && fgets(line, sizeof(line), tf))
		{
			std::string l = line;
			while (!l.empty() && (l.back() == '\n' || l.back() == '\r')) l.pop_back();
			if (l.empty() || l[0] == '#') continue;
			if (!l.compare(0, 7, "static|")) g_static = Unescape(l.substr(7));
			else g_texts.push_back(Unescape(l));
		}
		if (tf) fclose(tf);
		static char nframes[16];
		sprintf_s(nframes, "%d", (int)g_texts.size());
		argv[2] = nframes;
		static char nocap[] = "nocap", zero[] = "0";
		argc = 7;
		static char* args[8];
		for (int i = 0; i < 5; i++) args[i] = argv[i];
		args[5] = zero; args[6] = nocap; args[7] = NULL;
		argv = args;
	}
	if (watch) argv[3] = (char*)(new std::string(std::to_string(1000.0 / atof(argv[3]))))->c_str();
	int frames = atoi(argv[2]);
	double hz = atof(argv[3]);
	double seconds = atof(argv[4]);
	double fpsCap = argc > 5 ? atof(argv[5]) : 0;
	bool noCapture = argc > 6 && !strcmp(argv[6], "nocap");

	SetProcessDPIAware();
	QueryPerformanceFrequency(&g_freq);
	timeBeginPeriod(1);

	WNDCLASSA wc = { 0 };
	wc.lpfnWndProc = DefWindowProcA;
	wc.hInstance = GetModuleHandle(NULL);
	wc.lpszClassName = "RTSSAnimTest";
	wc.hCursor = LoadCursor(NULL, IDC_ARROW);
	RegisterClassA(&wc);
	int sx = (GetSystemMetrics(SM_CXSCREEN) - W) / 2, sy = (GetSystemMetrics(SM_CYSCREEN) - H) / 2;
	HWND hWnd = CreateWindowExA(WS_EX_TOPMOST | WS_EX_TOOLWINDOW, "RTSSAnimTest", "RTSS animation test", WS_POPUP | WS_VISIBLE, sx, sy, W, H, NULL, NULL, wc.hInstance, NULL);

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

	HANDLE hMap = OpenFileMappingA(FILE_MAP_ALL_ACCESS, FALSE, "RTSSSharedMemoryV2");
	LPRTSS_SHARED_MEMORY mem = hMap ? (LPRTSS_SHARED_MEMORY)MapViewOfFile(hMap, FILE_MAP_ALL_ACCESS, 0, 0, 0) : NULL;
	if (!mem || mem->dwSignature != 'RTSS') { printf("RTSS is not running\n"); return 1; }

	// Let RTSS hook the process, then show the first frame and let it load the atlas.
	for (double t0 = Now(); Now() - t0 < 2500;) Frame();
	if (!watch)
	{
		if (!g_static.empty()) WriteSlot(mem, "AnimTest_0", g_static);
		WriteSlot(mem, "AnimTest_1", TextFor(atlas, 0));
		WriteSlot(mem, "AnimTest_2", "<P=0,0><S=100>");
	}
	for (double t0 = Now(); Now() - t0 < 1000;) Frame();

	POINT org = { 0, 0 };
	ClientToScreen(hWnd, &org);
	HDC scr = GetDC(NULL);
	HDC memDC = CreateCompatibleDC(scr);
	BITMAPINFO bi = { 0 };
	bi.bmiHeader.biSize = sizeof(bi.bmiHeader);
	bi.bmiHeader.biWidth = SPRITE_W + 8; bi.bmiHeader.biHeight = -(SPRITE_H + 4);
	bi.bmiHeader.biPlanes = 1; bi.bmiHeader.biBitCount = 32;
	BYTE* px = NULL;
	HBITMAP bmp = CreateDIBSection(scr, &bi, DIB_RGB_COLORS, (void**)&px, NULL, 0);
	SelectObject(memDC, bmp);

	int first = Observe(scr, memDC, px, org);
	if (!watch && first != 0) { printf("frame 0 is not on screen before the run (saw %d): the bench can't read the sprite\n", first); }

	Writer w = { mem, atlas, frames, hz > 0 ? 1000.0 / hz : 1000.0, 0, 0, 0, 0, 0, 0 };
	HANDLE hThread = (watch || hz <= 0) ? NULL : CreateThread(NULL, 0, WriterThread, &w, 0, NULL);
	if (hThread) SetThreadPriority(hThread, THREAD_PRIORITY_TIME_CRITICAL);

	// What was seen: (time, frame), one entry per change.
	std::vector<std::pair<double, int>> seen;
	int last = -2, rendered = 0, samples = 0;
	double start = Now(), nextFrame = start, prevFrame = start, worst = 0;
	int over10 = 0, over5 = 0;
	while (Now() - start < seconds * 1000)
	{
		Frame();
		rendered++;
		double t = Now(), dt = t - prevFrame;
		prevFrame = t;
		if (dt > worst) worst = dt;
		if (dt > 10) over10++;
		if (dt > 5) over5++;
		int f = noCapture ? last : Observe(scr, memDC, px, org);
		samples++;
		if (f != last) { seen.push_back({ Now() - start, f }); last = f; }
		if (fpsCap > 0)
		{
			nextFrame += 1000.0 / fpsCap;
			while (Now() < nextFrame) { if (nextFrame - Now() > 2) Sleep(1); else YieldProcessor(); }
		}
	}
	double elapsed = Now() - start;
	w.stop = 1;
	if (hThread) WaitForSingleObject(hThread, 1000);

	// Walk the changes: each should be the next frame.
	int ok = 0, skipped = 0, backwards = 0, blanks = 0;
	std::vector<double> shown;
	for (size_t i = 1; i < seen.size(); i++)
	{
		int prev = seen[i - 1].second, cur = seen[i].second;
		if (cur < 0) { blanks++; continue; }
		if (prev < 0) continue;
		int step = ((cur - prev) % frames + frames) % frames;
		if (step == 1) ok++;
		else if (step <= frames / 2) skipped += step - 1;
		else backwards++;
		if (i + 1 < seen.size()) shown.push_back(seen[i + 1].first - seen[i].first);
	}
	double sum = 0, mn = 1e9, mx = 0, var = 0;
	for (double d : shown) { sum += d; if (d < mn) mn = d; if (d > mx) mx = d; }
	double avg = shown.empty() ? 0 : sum / shown.size();
	for (double d : shown) var += (d - avg) * (d - avg);
	double dev = shown.empty() ? 0 : sqrt(var / shown.size());

	printf("%5.0f Hz, game %4.0f fps (%s), longest frame %.1f ms, frames over 5 ms %d, over 10 ms %d: wrote %d (lock wait avg %.3f max %.2f ms, gave up %d) | seen %d changes: in order %d, skipped frames %d, backwards %d, blank %d | frame on screen %.1f ms ideal, avg %.1f min %.1f max %.1f sd %.1f\n",
		hz, rendered * 1000.0 / elapsed, fpsCap > 0 ? "capped" : "free", worst, over5, over10, w.writes, w.writes ? w.sumWait / w.writes : 0, w.maxWait, w.gaveUp,
		(int)seen.size() - 1, ok, skipped, backwards, blanks, 1000.0 / hz, avg, shown.empty() ? 0 : mn, mx, dev);
	fflush(stdout);

	if (!watch)
	{
		ReleaseSlot(mem, "AnimTest_0");
		ReleaseSlot(mem, "AnimTest_1");
		ReleaseSlot(mem, "AnimTest_2");
	}
	for (double t0 = Now(); Now() - t0 < 150;) Frame();
	timeEndPeriod(1);
	DestroyWindow(hWnd);
	return 0;
}
