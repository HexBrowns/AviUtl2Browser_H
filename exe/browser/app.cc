#include "app.h"

#include <string>

#include "include/base/cef_callback.h"
#include "include/base/cef_ref_counted.h"
#include "include/wrapper/cef_closure_task.h"
#include "include/wrapper/cef_helpers.h"

#include "callback.h"

// AviUtl2Browser_H: 終了のとき、ブラウザが閉じきるのをこれ以上は待たない。
// .mod2 は標準入力を閉じてから 3 秒待って強制終了する（src/process.rs の SHUTDOWN_WAIT）ので、それより短くする
static const int64_t kCloseBrowsersTimeoutMs = 2000;

App::App() : initialized_(false), browser_count_(0), quitting_(false), quit_called_(false) {
}

void App::WaitInitialize() {
	std::unique_lock<std::mutex> lk(mtx_);
	cv_.wait(lk, [&] {
		return initialized_ == true;
	});
}

bool App::RenderAndCapture(
    const std::wstring& path,
    const std::wstring& tabid,
    const bool use_dir,
    const bool use_devtools,
    const std::wstring& userfile,
    const std::wstring& inbuf,
    std::wstring& outbuf,
    void* image, const int width, const int height) {
	std::lock_guard<std::mutex> lk(mtx_);
    CefRefPtr<Client> c;
    const auto key = client_key{ path, tabid, use_dir };
    const auto it = clients_.find(key);
    if (it != clients_.cend()) {
        c = it->second;
    } else {
        c = new Client(InitAssets(path, use_dir), this);
        clients_[key] = c;
        CefWindowInfo window_info;
        window_info.SetAsWindowless(nullptr);
        CefBrowserSettings browser_settings;
        browser_settings.background_color = 0;
        // AviUtl2Browser_H: 画面外描画の上限（既定 30）を上げる。1 回の描画は合図の色が乗った絵を待つので、
        // 上限が 1 回あたりの時間をほぼ決める（FHD で 30 のとき約 33ms）
        browser_settings.windowless_frame_rate = 60;
        CefString utf8(L"UTF-8");
        cef_string_utf16_set(utf8.c_str(), utf8.length(), &browser_settings.default_encoding, false);
        browser_settings.javascript_access_clipboard = STATE_DISABLED;
        browser_settings.javascript_dom_paste = STATE_DISABLED;
        CefBrowserHost::CreateBrowser(
            window_info,
            c,
            L"https://assets.example/",
            browser_settings,
            nullptr,
            nullptr
        );
        c->Wait();
    }
    c->SetUseDevTools(use_devtools);
    c->SetUserFile(userfile);
    return c->RenderAndCapture(inbuf, outbuf, image, width, height);
}

// ipc_worker のスレッドから呼ぶ（標準入力が閉じられた＝ .mod2 が外れた・本体が終了した）
void App::CloseAllBrowsers() {
	std::lock_guard<std::mutex> lk(mtx_);
    for (auto &c : clients_) {
        c.second->CloseAllBrowsers(true);
    }
    clients_.clear();
	// AviUtl2Browser_H: 原作はここで CefQuitMessageLoop を積んでいた。CloseBrowser は非同期に閉じることがあり、
	// ブラウザが閉じきる前にメッセージループを抜けて CefShutdown へ進んでいた。
	// CEF の定石どおり、最後のブラウザの OnBeforeClose で抜ける（cef_life_span_handler.h の DoClose の説明）。
	// 上の閉じる依頼は UI スレッドへ積まれているので、この後に積む QuitWhenAllClosed はそれらの後に走る
	CefPostTask(TID_UI, base::BindOnce(&App::QuitWhenAllClosed, this));
}

void App::QuitWhenAllClosed() {
	CEF_REQUIRE_UI_THREAD();
	quitting_ = true;
	if (browser_count_ <= 0) {
		// ブラウザが 1 つも無い（一度も描いていない・すでに閉じきった）ならそのまま抜ける
		QuitMessageLoopOnce();
		return;
	}
	// OnBeforeClose が来ないまま待ち続けないよう、上限を付ける
	CefPostDelayedTask(TID_UI, base::BindOnce(&App::QuitMessageLoopOnce, this), kCloseBrowsersTimeoutMs);
}

void App::QuitMessageLoopOnce() {
	CEF_REQUIRE_UI_THREAD();
	if (quit_called_) {
		return;
	}
	quit_called_ = true;
	CefQuitMessageLoop();
}

void App::OnBrowserCreated() {
	CEF_REQUIRE_UI_THREAD();
	++browser_count_;
}

void App::OnBrowserClosed() {
	CEF_REQUIRE_UI_THREAD();
	--browser_count_;
	if (quitting_ && browser_count_ <= 0) {
		QuitMessageLoopOnce();
	}
}

void App::OnContextInitialized() {
	CEF_REQUIRE_UI_THREAD();
    initialized_ = true;
    cv_.notify_all();
}

// AviUtl2Browser_H: 同じ root_cache_path の CEF が先に動いていると、後から起動した exe の引数がここへ回される。
// 既定の動き（Chrome の窓を新しく開く）をさせない。キャッシュの場所は exe ごとに分けている（aviutl_browser_win.cc）ので、
// ここへ来るのは古い版の exe が残っているときなどに限られる
bool App::OnAlreadyRunningAppRelaunch(CefRefPtr<CefCommandLine> command_line, const CefString& current_directory) {
	return true;
}

// AviUtl2Browser_H: 原作は OnWebKitInitialized で CefRegisterExtension（V8 拡張と `native function`）を使い、
// ここで window.AviUtlBrowser に代入していた。CEF 154 で CefRegisterExtension が無くなったので、
// 同じ JS を「rendered を受け取る関数」として評価し、CefV8Handler の rendered を渡して作る。
// JS から見える AviUtlBrowser（render / registerRenderer）は原作と同じ。
static const wchar_t* const kAviUtlBrowserFactory = LR"JS((rendered) => {
  "use strict";
  let renderer = null;
  let ackElem = null;
  let curAck = 0;
  function updateAck() {
    if (document.body && !ackElem) {
      ackElem = document.createElement('div');
      ackElem.style.display = 'block';
      ackElem.style.position = 'fixed';
      ackElem.style.zIndex = '2147483647';
      ackElem.style.left = '0';
      ackElem.style.top = '0';
      ackElem.style.width = '1px';
      ackElem.style.height = '1px';
    }
    if (!ackElem) {
      return 0;
    }
    if (!ackElem.parentNode) {
      document.body.appendChild(ackElem);
    }
    curAck >>= 8;
    if (!curAck) {
      curAck = 0xff0000;
    }
    ackElem.style.backgroundColor = `rgb(${(curAck >>> 16) & 0xff}, ${(curAck >>> 8) & 0xff}, ${(curAck >>> 0) & 0xff})`;
    return curAck;
  }
  return {
    render(idx, param, userfile) {
      if (renderer) {
        renderer({param, userfile}).then(param => {
          rendered(idx, updateAck(), true, param);
        }).catch(err => {
          rendered(idx, updateAck(), false, err);
        });
      } else {
        updateAck();
        rendered(idx, updateAck(), false, "renderer is not registered");
      }
    },
    registerRenderer(fn) {
      renderer = fn;
    },
  };
})JS";

void App::OnContextCreated(CefRefPtr<CefBrowser> browser, CefRefPtr<CefFrame> frame, CefRefPtr<CefV8Context> context) {
    CefRefPtr<CefV8Value> factory;
    CefRefPtr<CefV8Exception> err;
    if (!context->Eval(kAviUtlBrowserFactory, frame->GetURL(), 0, factory, err) || !factory || !factory->IsFunction()) {
        return;
    }
    CefV8ValueList args;
    args.push_back(CefV8Value::CreateFunction("rendered", new CallbackHandler()));
    CefRefPtr<CefV8Value> api = factory->ExecuteFunctionWithContext(context, nullptr, args);
    if (!api || !api->IsObject()) {
        return;
    }
    if (context->Enter()) {
        context->GetGlobal()->SetValue("AviUtlBrowser", api, V8_PROPERTY_ATTRIBUTE_NONE);
        context->Exit();
    }
}

void App::OnWebKitInitialized() {
}
