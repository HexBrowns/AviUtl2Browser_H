#pragma once

#include <map>
#include <mutex>
#include <condition_variable>

#include "include/cef_app.h"

#include "client.h"

struct client_key {
	std::wstring path;
	std::wstring tabid;
	bool is_dir;
	bool operator<(const client_key& value) const
	{
		// AviUtl2Browser_H: 原作は左辺の tabid が value.tabid になっていて、tabid が比較されず別タブにならなかった
		return std::tie(path, is_dir, tabid) < std::tie(value.path, value.is_dir, value.tabid);
	}
};

class App : public CefApp, public CefBrowserProcessHandler, public CefRenderProcessHandler, public BrowserObserver {
public:
	explicit App();

	// CefApp methods:
	virtual CefRefPtr<CefBrowserProcessHandler> GetBrowserProcessHandler() override {
		return this;
	}
	virtual CefRefPtr<CefRenderProcessHandler> GetRenderProcessHandler() override {
		return this;
	}

	// CefBrowserProcessHandler methods:
	virtual void OnContextInitialized() override;
	virtual bool OnAlreadyRunningAppRelaunch(CefRefPtr<CefCommandLine> command_line, const CefString& current_directory) override;

	// BrowserObserver methods（UI スレッド）:
	virtual void OnBrowserCreated() override;
	virtual void OnBrowserClosed() override;

	// CefRenderProcessHandler methods:
	virtual void OnWebKitInitialized() override;
	virtual void OnContextCreated(CefRefPtr<CefBrowser> browser, CefRefPtr<CefFrame> frame, CefRefPtr<CefV8Context> context) override;

	void WaitInitialize();
	bool RenderAndCapture(
		const std::wstring& path,
		const std::wstring& tabid,
		const bool use_dir,
		const bool use_devtools,
		const std::wstring& userfile,
		const std::wstring& inbuf,
		std::wstring& outbuf,
		void* image, const int width, const int height
	);
	void CloseAllBrowsers();
private:
	std::mutex mtx_;
	std::condition_variable cv_;
	bool initialized_;

	std::map<client_key, CefRefPtr<Client>> clients_;

	// AviUtl2Browser_H: 終了の段取り。どれも UI スレッドだけで触る
	int browser_count_;   // 生きているブラウザ（DevTools の窓を含む）
	bool quitting_;       // CloseAllBrowsers の後。最後のブラウザが閉じたら抜ける
	bool quit_called_;    // CefQuitMessageLoop を 1 回だけ呼ぶ
	void QuitWhenAllClosed();
	void QuitMessageLoopOnce();

	CefRefPtr<Assets> InitAssets(const std::wstring& path, const bool use_dir);

	IMPLEMENT_REFCOUNTING(App);
};
