import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/shell.dart';

void main() {
  test('hostFriendlyUrl shows the host and drops a leading www', () {
    expect(hostFriendlyUrl('https://www.linkedin.com/'), 'linkedin.com');
    expect(hostFriendlyUrl('https://linkedin.com'), 'linkedin.com');
  });

  test('hostFriendlyUrl keeps a non-root path', () {
    expect(
      hostFriendlyUrl('https://www.linkedin.com/feed/'),
      'linkedin.com/feed/',
    );
  });
}
