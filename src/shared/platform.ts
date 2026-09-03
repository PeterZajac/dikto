// WKWebView reports "Macintosh", WebView2 reports "Windows NT".
export const isMac = /Macintosh|Mac OS X/.test(navigator.userAgent);
