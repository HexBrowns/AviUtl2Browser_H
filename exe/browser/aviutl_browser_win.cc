#include <thread>
#include <Windows.h>

#include "include/cef_parser.h"

#include "app.h"
#include "client.h"
#include "unicode_win.h"
#include "path_win.h"
#include "report_win.h"
#include "version.h"

// AviUtl2Browser_H: 原作は CEF_USE_SANDBOX で cef_sandbox.lib をリンクしていた。
// CEF 138 以降は bootstrap.exe 方式に変わったため、サンドボックスは使わない（CMakeLists.txt）。

// 共有メモリの先頭（原作の bridge.dll と同じ形）。本体は header_size の後ろに BGRA で w*h*4 バイト
struct share_mem_header {
	uint32_t header_size;
	uint32_t body_size;
	uint32_t version;
	uint32_t width;
	uint32_t height;
};

// 要求のヘッダー。原作は版 2（SJIS、共有メモリの名前は起動時の環境変数 BRIDGE_FMO）。
// AviUtl2Browser_H は版 3: 文字列は UTF-8、共有メモリの名前を毎回載せる（大きさが変わるたびに作り直すため）。
// 続く文字列の順: path, userfile, tabid, fmo_name, param（param は残り全部）
#pragma pack(push, 1)
struct header {
	uint8_t version;
	uint8_t flags;
	uint16_t path_len;
	uint16_t userfile_len;
	uint16_t tabid_len;
	uint16_t fmo_name_len;
};
#pragma pack(pop)
static_assert(sizeof(header) == 10, "header layout");

inline static BOOL read(HANDLE h, void* buf, DWORD sz)
{
	char* b = (char*)buf;
	for (DWORD read(0); sz > 0; b += read, sz -= read)
	{
		if (!ReadFile(h, b, sz, &read, NULL))
		{
			return FALSE;
		}
		if (read == 0) {
			return FALSE;
		}
	}
	return TRUE;
}

inline static BOOL write(HANDLE h, const void* buf, DWORD sz)
{
	const char* b = (char*)buf;
	for (DWORD written(0); sz > 0; b += written, sz -= written) {
		if (!WriteFile(h, b, sz, &written, NULL))
		{
			return FALSE;
		}
	}
	return TRUE;
}

// 共有メモリを名前で開き、名前が変わったときだけ開き直す
class SharedImage {
public:
	~SharedImage() { Close(); }
	share_mem_header* Open(const std::string& name) {
		if (view_ && name == name_) {
			return view_;
		}
		Close();
		fmo_ = OpenFileMappingA(FILE_MAP_ALL_ACCESS, FALSE, name.c_str());
		if (!fmo_) {
			report(HRESULT_FROM_WIN32(GetLastError()), L"OpenFileMapping failed");
			return nullptr;
		}
		view_ = (share_mem_header*)MapViewOfFile(fmo_, FILE_MAP_WRITE, 0, 0, 0);
		if (!view_) {
			report(HRESULT_FROM_WIN32(GetLastError()), L"MapViewOfFile failed");
			CloseHandle(fmo_);
			fmo_ = NULL;
			return nullptr;
		}
		name_ = name;
		return view_;
	}
	void Close() {
		if (view_) {
			UnmapViewOfFile(view_);
			view_ = nullptr;
		}
		if (fmo_) {
			CloseHandle(fmo_);
			fmo_ = NULL;
		}
		name_.clear();
	}
private:
	HANDLE fmo_ = NULL;
	share_mem_header* view_ = nullptr;
	std::string name_;
};

static BOOL respond(HANDLE out, const bool ok, const std::string& body) {
	int32_t len = 1 + (int32_t)body.size();
	if (!write(out, &len, sizeof(len))) {
		return FALSE;
	}
	const char b = ok ? 1 : 0;
	if (!write(out, &b, sizeof(b))) {
		return FALSE;
	}
	if (!body.empty()) {
		if (!write(out, &body[0], static_cast<DWORD>(body.size()))) {
			return FALSE;
		}
	}
	return TRUE;
}

static void utf8_to_wide(const char* s, const size_t len, std::wstring& o) {
	if (FAILED(to_u16(s, static_cast<int>(len), o))) {
		o.clear();
	}
}

static int ipc_worker(CefRefPtr<App> app) {
	SharedImage shm;
	HANDLE in = GetStdHandle(STD_INPUT_HANDLE);
	HANDLE out = GetStdHandle(STD_OUTPUT_HANDLE);
	if (in == INVALID_HANDLE_VALUE || in == NULL || out == INVALID_HANDLE_VALUE || out == NULL) {
		OutputDebugString(L"standard handles are not available");
		goto finish;
	}
	app->WaitInitialize();

	{
		std::string buf, outbuf;
		std::wstring path, userfile, tabid, win, wout;
		int32_t len(0);
		while (1) {
			// 標準入力が閉じられたら（AviUtl2Browser_H.mod2 が外れた・本体が終了した）終わる
			if (!read(in, &len, sizeof(len)) || len < 0) {
				goto finish;
			}
			buf.resize(len);
			if (len > 0 && !read(in, &buf[0], len)) {
				goto finish;
			}
			if (buf.size() < sizeof(header)) {
				if (!respond(out, false, "request is too short")) {
					goto finish;
				}
				continue;
			}
			const header* h = reinterpret_cast<const header*>(&buf[0]);
			if (h->version != 3) {
				if (!respond(out, false, "unsupported request version")) {
					goto finish;
				}
				continue;
			}
			const size_t strings = static_cast<size_t>(h->path_len) + h->userfile_len + h->tabid_len + h->fmo_name_len;
			if (sizeof(header) + strings > buf.size()) {
				if (!respond(out, false, "request is broken")) {
					goto finish;
				}
				continue;
			}
			size_t pos = sizeof(header);
			utf8_to_wide(&buf[pos], h->path_len, path);
			pos += h->path_len;
			utf8_to_wide(&buf[pos], h->userfile_len, userfile);
			pos += h->userfile_len;
			utf8_to_wide(&buf[pos], h->tabid_len, tabid);
			pos += h->tabid_len;
			const std::string fmo_name(&buf[pos], h->fmo_name_len);
			pos += h->fmo_name_len;
			utf8_to_wide(&buf[pos], buf.size() - pos, win);

			share_mem_header* view = shm.Open(fmo_name);
			if (!view) {
				if (!respond(out, false, "cannot open shared memory")) {
					goto finish;
				}
				continue;
			}
			if (static_cast<uint64_t>(view->width) * view->height * 4 > view->body_size || view->width == 0 || view->height == 0) {
				if (!respond(out, false, "shared memory is too small")) {
					goto finish;
				}
				continue;
			}
			wout.clear();
			const bool r = app->RenderAndCapture(
				path,
				tabid,
				(h->flags & 1) != 0,
				(h->flags & 2) != 0,
				userfile,
				win,
				wout,
				(void*)(((char*)view) + view->header_size), view->width, view->height);
			if (FAILED(to_u8(wout.c_str(), static_cast<int>(wout.size()), outbuf))) {
				outbuf.clear();
			}
			if (!respond(out, r, outbuf)) {
				goto finish;
			}
		}
	}

finish:
	shm.Close();
	app->CloseAllBrowsers();
	return 1;
}

// AviUtl2Browser_H: キャッシュの場所を exe ごとに分ける。
// AviUtl2 を 2 つ起動すると exe も 2 つ動く。CEF 120 以降は root_cache_path ごとに 1 プロセスしか動けず
// （cef_types.h の root_cache_path、cef_app.h の CefInitialize）、後から起動した側の CefInitialize が false を返して終わる。
// 1 つ目は今までどおり exe\cache を使い（ログインやサイトの設定を引き継ぐ）、使用中なら exe\cache_2、cache_3 … を順に試す。
// 使用中かは、各フォルダの AviUtl2Browser_H.lock を共有なしで開けるかで決める。開いたハンドルはプロセスの終わりまで持ち、
// 落ちたときも OS が閉じる。番号を使い回すので、フォルダは同時に動かした数より増えない
static const int kMaxCacheSlots = 16;

static bool create_dir(const std::wstring& s) {
	if (!CreateDirectoryW(s.c_str(), NULL)) {
		const DWORD err = GetLastError();
		if (err != ERROR_ALREADY_EXISTS) {
			report(HRESULT_FROM_WIN32(err), L"CreateDirectory failed");
			return false;
		}
	}
	return true;
}

static bool create_cache_dir(std::wstring& o) {
	std::wstring base;
	if (!get_module_file_name(NULL, base)) {
		return false;
	}
	for (auto i = base.size() - 1; i > 0; --i) {
		if ((base[i] == L'/' || base[i] == L'\\')) {
			base.resize(i + 1);
			break;
		}
	}
	for (int slot = 1; slot <= kMaxCacheSlots; ++slot) {
		std::wstring s = base + L"cache";
		if (slot > 1) {
			s += L"_" + std::to_wstring(slot);
		}
		if (!create_dir(s)) {
			return false;
		}
		const std::wstring lock_path = s + L"\\AviUtl2Browser_H.lock";
		// 意図して閉じない（プロセスの終わりまで持つ）
		const HANDLE lock = CreateFileW(lock_path.c_str(), GENERIC_READ | GENERIC_WRITE, 0, NULL, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
		if (lock == INVALID_HANDLE_VALUE) {
			const DWORD err = GetLastError();
			if (err == ERROR_SHARING_VIOLATION) {
				continue; // 別の exe が使っている
			}
			report(HRESULT_FROM_WIN32(err), L"CreateFile(cache lock) failed");
			return false;
		}
		o = s;
		return true;
	}
	report(HRESULT_FROM_WIN32(ERROR_BUSY), L"all cache folders are in use");
	return false;
}

int APIENTRY wWinMain(HINSTANCE hInstance,
	HINSTANCE hPrevInstance,
	LPTSTR lpCmdLine,
	int nCmdShow) {
	UNREFERENCED_PARAMETER(hPrevInstance);
	UNREFERENCED_PARAMETER(lpCmdLine);

	std::thread t1;
	{
		CefMainArgs main_args(hInstance);
		CefRefPtr<App> app(new App);

		int exit_code = CefExecuteProcess(main_args, app, nullptr);
		if (exit_code >= 0) {
			return exit_code;
		}

		CefSettings settings;
		settings.windowless_rendering_enabled = true;
		settings.no_sandbox = true;

		std::wstring cache;
		if (!create_cache_dir(cache)) {
			return 1;
		}
		CefString(&settings.root_cache_path).FromWString(cache);
		CefString(&settings.cache_path).FromWString(cache);

		std::wstring ver(L"AviUtlBrowser/");
		ver += version;
		CefString(&settings.user_agent_product).FromWString(ver);

		LANGID langId = GetUserDefaultLangID();
		wchar_t lang[32] = {}, ctry[32] = {};
		if (!GetLocaleInfo(langId, LOCALE_SISO639LANGNAME, lang, 32)) {
			report(HRESULT_FROM_WIN32(GetLastError()), L"GetLocaleInfo failed");
			return 1;
		}
		if (!GetLocaleInfo(langId, LOCALE_SISO3166CTRYNAME, ctry, 32)) {
			report(HRESULT_FROM_WIN32(GetLastError()), L"GetLocaleInfo failed");
			return 1;
		}
		std::wstring locale(lang);
		locale += L"-";
		locale += std::wstring(ctry);
		CefString(&settings.locale).FromWString(locale);
		CefString(&settings.accept_language_list).FromWString(locale);

		if (!CefInitialize(main_args, settings, app.get(), nullptr)) {
			return CefGetExitCode();
		}
		t1 = std::thread(ipc_worker, app);
	}

	CefRunMessageLoop();
	t1.join();
	CefShutdown();
	return 0;
}
