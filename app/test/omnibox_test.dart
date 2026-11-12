import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/omnibox.dart';

String ddg(String q) => Uri.https('duckduckgo.com', '/', {'q': q}).toString();

void main() {
  test('a phrase is a DuckDuckGo search, not a percent-encoded URL', () {
    const phrase = 'what do i need to install to get youtube working';
    expect(omniboxTarget(phrase), ddg(phrase));
    expect(
      omniboxTarget('  how to fix example.com  '),
      ddg('how to fix example.com'),
    );
    expect(omniboxTarget('flutter'), ddg('flutter'));
    expect(omniboxTarget('c++'), ddg('c++'));
    expect(omniboxTarget('what is 2.5'), ddg('what is 2.5'));
  });

  test('addresses open directly', () {
    expect(omniboxTarget('instagram.com'), 'https://instagram.com');
    expect(
      omniboxTarget('www.youtube.com/watch?v=x'),
      'https://www.youtube.com/watch?v=x',
    );
    expect(omniboxTarget('example.com:8080/a'), 'https://example.com:8080/a');
    expect(
      omniboxTarget('https://example.com/a b'),
      ddg('https://example.com/a b'),
    );
    expect(omniboxTarget('http://example.com'), 'http://example.com');
    expect(omniboxTarget('HTTPS://Example.com/x'), 'HTTPS://Example.com/x');
    expect(omniboxTarget('localhost:3000'), 'http://localhost:3000');
    expect(omniboxTarget('192.168.1.1/admin'), 'http://192.168.1.1/admin');
    expect(omniboxTarget('about:blank'), 'about:blank');
    expect(omniboxTarget('file:///tmp/a.html'), 'file:///tmp/a.html');
  });

  test('unsafe or unknown schemes search instead of running', () {
    expect(omniboxTarget('javascript:alert(1)'), ddg('javascript:alert(1)'));
    expect(omniboxTarget('https://'), ddg('https://'));
  });

  test('blank input does nothing', () {
    expect(omniboxTarget(''), isNull);
    expect(omniboxTarget('   '), isNull);
  });
}
