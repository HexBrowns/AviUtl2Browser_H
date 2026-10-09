#
# aviutl_browser.exe のソース（原作 oov/aviutl_browser の src/exe。Windows 以外の分岐は外した）
#

set(AVIUTLBROWSER_SRCS
  miniz.c
  assets.cc
  assets.h
  app.cc
  app.h
  client.cc
  client.h
  callback.h
  depremultipliedalpha_table.h
  picojson.h
  aviutl_browser.exe.manifest
  aviutl_browser.rc
  aviutl_browser_win.cc
  app_win.cc
  client_win.cc
  assets_win.cc
  assets_win.h
  path_win.cc
  path_win.h
  resource.h
  report_win.cc
  report_win.h
  unicode_win.h
  )
source_group(aviutl_browser FILES ${AVIUTLBROWSER_SRCS})

set(CEF_TARGET "aviutl_browser")
ADD_LOGICAL_TARGET("libcef_lib" "${CEF_LIB_DEBUG}" "${CEF_LIB_RELEASE}")
SET_CEF_TARGET_OUT_DIR()

add_executable(${CEF_TARGET} WIN32 ${AVIUTLBROWSER_SRCS})
add_dependencies(${CEF_TARGET} libcef_dll_wrapper)
SET_EXECUTABLE_TARGET_PROPERTIES(${CEF_TARGET})
target_link_libraries(${CEF_TARGET} libcef_lib libcef_dll_wrapper ${CEF_STANDARD_LIBS})

ADD_WINDOWS_MANIFEST("${CMAKE_CURRENT_SOURCE_DIR}" "${CEF_TARGET}" "exe")

COPY_FILES("${CEF_TARGET}" "${CEF_BINARY_FILES}" "${CEF_BINARY_DIR}" "${CEF_TARGET_OUT_DIR}")
COPY_FILES("${CEF_TARGET}" "${CEF_RESOURCE_FILES}" "${CEF_RESOURCE_DIR}" "${CEF_TARGET_OUT_DIR}")

configure_file("${CMAKE_CURRENT_SOURCE_DIR}/version.h.in" "${CMAKE_CURRENT_BINARY_DIR}/version.h" @ONLY NEWLINE_STYLE LF)
target_include_directories(${CEF_TARGET} PRIVATE "${CMAKE_CURRENT_BINARY_DIR}")
# 日本語のコメントを含むので UTF-8 として読ませる（既定は CP932 で、行末の 0x5C が行の継続になりうる）
target_compile_options(${CEF_TARGET} PRIVATE /utf-8)
