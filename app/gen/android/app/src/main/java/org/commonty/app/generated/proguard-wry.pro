# THIS FILE IS AUTO-GENERATED. DO NOT MODIFY!!

# Copyright 2020-2023 Tauri Programme within The Commons Conservancy
# SPDX-License-Identifier: Apache-2.0
# SPDX-License-Identifier: MIT

-keep class org.commonty.app.* {
  native <methods>;
}

-keep class org.commonty.app.WryActivity {
  public <init>(...);

  void setWebView(org.commonty.app.RustWebView);
  java.lang.Class getAppClass(...);
  int getId();
  java.lang.String getVersion();
  int startActivity(...);
}

-keep class org.commonty.app.Ipc {
  public <init>(...);

  @android.webkit.JavascriptInterface public <methods>;
}

-keep class org.commonty.app.RustWebView {
  public <init>(...);

  void loadUrlMainThread(...);
  void loadHTMLMainThread(...);
  void evalScript(...);
}

-keep class org.commonty.app.RustWebChromeClient,org.commonty.app.RustWebViewClient {
  public <init>(...);
}
