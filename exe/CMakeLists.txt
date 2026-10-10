# Copyright (c) 2014 The Chromium Embedded Framework Authors. All rights
# reserved. Use of this source code is governed by a BSD-style license that
# can be found in the LICENSE file.
#
# aviutl_browser.exe（AviUtl2Browser_H の描画プロセス）
#
# 原作 oov/aviutl_browser の src/CMakeLists.txt（CEF のサンプルの CMake が元）を短くした。
# 原作は CEF をビルド時にダウンロードしていたが、ここでは third_party/ に展開済みの
# CEF バイナリ配布物（minimal）を使う。取得の手順は ../README.md。
#
#   cmake -S . -B build -G "Visual Studio 17 2022" -A x64
#   cmake --build build --config Release --target aviutl_browser
#
# 出力は build/browser/Release/。CEF の実行時ファイルも同じ場所へ写される。

cmake_minimum_required(VERSION 3.21)

set(CMAKE_CONFIGURATION_TYPES Debug Release)

project(aviutl_browser)

set_property(GLOBAL PROPERTY OS_FOLDERS ON)

# CEF の版を変えたら、この名前と ../README.md を合わせる
set(CEF_DIST_NAME "cef_binary_154.0.34+g14c5a08+chromium-154.0.8037.98_windows64_minimal")
set(CEF_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/third_party/${CEF_DIST_NAME}")
if(NOT EXISTS "${CEF_ROOT}/cmake/FindCEF.cmake")
  message(FATAL_ERROR "CEF binary distribution not found: ${CEF_ROOT} (see ../README.md)")
endif()
set(CMAKE_MODULE_PATH ${CMAKE_MODULE_PATH} "${CEF_ROOT}/cmake")

# CEF 138 以降、Windows のサンドボックスはクライアントを DLL にして bootstrap.exe から読ませる方式になった。
# 原作の exe の形を保つため、サンドボックスは使わない（仕様書「exe 側の差分」）
set(USE_SANDBOX OFF CACHE BOOL "" FORCE)

# 版（version.h に入る）。Cargo.toml の version と揃える
set(AVIUTL2BROWSER_VERSION "0.1.1" CACHE STRING "")

find_package(CEF REQUIRED)

add_subdirectory(${CEF_LIBCEF_DLL_WRAPPER_PATH} libcef_dll_wrapper)

add_subdirectory(browser)

PRINT_CEF_CONFIG()
