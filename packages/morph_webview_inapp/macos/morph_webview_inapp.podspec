Pod::Spec.new do |s|
  s.name             = 'morph_webview_inapp'
  s.version          = '0.1.0'
  s.summary          = 'Per-profile WKWebsiteDataStore hook for Morph.'
  s.description      = <<-DESC
Hooks WKWebView creation so macOS 14+ uses WKWebsiteDataStore(forIdentifier:)
for the current Morph profile.
                       DESC
  s.homepage         = 'https://github.com/WeirdBrains/morph'
  s.license          = { :file => '../LICENSE' }
  s.author           = { 'Weird Brains' => 'hello@weirdbrains.com' }
  s.source           = { :path => '.' }
  s.source_files     = 'Classes/**/*'
  s.dependency 'FlutterMacOS'
  s.platform = :osx, '10.14'
  s.pod_target_xcconfig = { 'DEFINES_MODULE' => 'YES' }
  s.swift_version = '5.0'
end
