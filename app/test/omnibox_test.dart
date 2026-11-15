import 'package:flutter_test/flutter_test.dart';

import 'support/omnibox_rule.dart';

String ddg(String q) => Uri.https('duckduckgo.com', '/', {'q': q}).toString();

/// The same cases as `crates/morph-core/src/omnibox.rs`, so the fake's rule
/// matches the one the app uses.
void main() {
  test('a phrase is a DuckDuckGo search, not a percent-encoded URL', () {
    const phrase = 'what do i need to install to get youtube working';
    expect(omniboxRule(phrase), ddg(phrase));
    expect(
      omniboxRule('  how to fix example.com  '),
      ddg('how to fix example.com'),
    );
    expect(omniboxRule('flutter'), ddg('flutter'));
    expect(omniboxRule('c++'), ddg('c++'));
    expect(omniboxRule('what is 2.5'), ddg('what is 2.5'));
  });

  test('addresses open directly', () {
    expect(omniboxRule('instagram.com'), 'https://instagram.com');
    expect(
      omniboxRule('www.youtube.com/watch?v=x'),
      'https://www.youtube.com/watch?v=x',
    );
    expect(omniboxRule('example.com:8080/a'), 'https://example.com:8080/a');
    expect(
      omniboxRule('https://example.com/a b'),
      ddg('https://example.com/a b'),
    );
    expect(omniboxRule('http://example.com'), 'http://example.com');
    expect(omniboxRule('HTTPS://Example.com/x'), 'HTTPS://Example.com/x');
    expect(omniboxRule('localhost:3000'), 'http://localhost:3000');
    expect(omniboxRule('192.168.1.1/admin'), 'http://192.168.1.1/admin');
    expect(omniboxRule('about:blank'), 'about:blank');
    expect(omniboxRule('file:///tmp/a.html'), 'file:///tmp/a.html');
  });

  test('unsafe or unknown schemes search instead of running', () {
    expect(omniboxRule('javascript:alert(1)'), ddg('javascript:alert(1)'));
    expect(omniboxRule('https://'), ddg('https://'));
  });

  test('blank input does nothing', () {
    expect(omniboxRule(''), isNull);
    expect(omniboxRule('   '), isNull);
  });
}
