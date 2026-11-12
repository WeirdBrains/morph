import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:morph/src/tabs/tab_position.dart';
import 'package:morph/src/tabs/tab_set.dart';
import 'package:morph/src/theme/morph_theme.dart';
import 'package:morph/src/widgets/desktop_chrome.dart';
import 'package:morph/src/window/window_buttons.dart';

Widget _harness({
  required MorphTabSet tabs,
  required MorphTabPosition position,
  TargetPlatform platform = TargetPlatform.linux,
  MorphWindowButtonMode mode = MorphWindowButtonMode.all,
  bool useNativeMacButtons = true,
  ValueChanged<MorphTabPosition>? onPositionChanged,
}) {
  return MaterialApp(
    theme: MorphTheme.dark,
    home: Scaffold(
      body: MorphDesktopChrome(
        tabs: tabs.tabs,
        activeId: tabs.activeId,
        position: position,
        buttonMode: mode,
        platform: platform,
        useNativeMacButtons: useNativeMacButtons,
        navigation: const SizedBox(
          key: Key('nav-row'),
          height: 48,
          child: Row(
            children: [
              Icon(Icons.arrow_back, key: Key('nav-back')),
              Expanded(child: TextField(key: Key('address-bar'))),
            ],
          ),
        ),
        content: const ColoredBox(
          key: Key('web-content'),
          color: Colors.black,
          child: SizedBox.expand(),
        ),
        onPositionChanged: onPositionChanged,
      ),
    ),
  );
}

void _desktopSize(WidgetTester tester) {
  final view = tester.view;
  view.physicalSize = const Size(1280, 800);
  view.devicePixelRatio = 1.0;
  addTearDown(view.reset);
}

void main() {
  testWidgets('top: tab strip above nav, nav above content, tabs not flipped', (
    tester,
  ) async {
    _desktopSize(tester);
    final tabs = MorphTabSet()..update(1, title: 'One');
    await tester.pumpWidget(
      _harness(tabs: tabs, position: MorphTabPosition.top),
    );

    final strip = tester.getRect(find.byKey(const Key('tab-strip')));
    final nav = tester.getRect(find.byKey(const Key('nav-row')));
    final address = tester.getRect(find.byKey(const Key('address-bar')));
    final content = tester.getRect(find.byKey(const Key('web-content')));

    expect(strip.bottom, lessThanOrEqualTo(nav.top + 0.5));
    expect(nav.bottom, lessThanOrEqualTo(content.top + 0.5));
    expect(address.top, lessThan(content.top));
    expect(find.byKey(const Key('caption-bar')), findsNothing);
    expect(find.byKey(const Key('tab-list')), findsNothing);
    expect(find.byKey(const Key('window-close')), findsOneWidget);
    expect(
      tester.getRect(find.byKey(const Key('window-close'))).bottom,
      lessThanOrEqualTo(nav.top + 0.5),
    );

    final chip = tester.widget<MorphTabChip>(find.byType(MorphTabChip));
    expect(chip.flipped, isFalse);
    expect(find.text('One'), findsOneWidget);
  });

  testWidgets('bottom: nav at top, flipped tabs below content', (tester) async {
    _desktopSize(tester);
    final tabs = MorphTabSet()..update(1, title: 'One');
    await tester.pumpWidget(
      _harness(tabs: tabs, position: MorphTabPosition.bottom),
    );

    final caption = tester.getRect(find.byKey(const Key('caption-bar')));
    final nav = tester.getRect(find.byKey(const Key('nav-row')));
    final content = tester.getRect(find.byKey(const Key('web-content')));
    final strip = tester.getRect(find.byKey(const Key('tab-strip')));

    expect(caption.bottom, lessThanOrEqualTo(nav.top + 0.5));
    expect(nav.top, lessThan(content.top));
    expect(nav.bottom, lessThanOrEqualTo(content.top + 0.5));
    expect(content.bottom, lessThanOrEqualTo(strip.top + 0.5));
    expect(find.byKey(const Key('tab-list')), findsNothing);
    expect(find.byKey(const Key('window-close')), findsOneWidget);
    expect(
      tester.getRect(find.byKey(const Key('window-close'))).bottom,
      lessThanOrEqualTo(nav.top + 0.5),
    );

    final chip = tester.widget<MorphTabChip>(find.byType(MorphTabChip));
    expect(chip.flipped, isTrue);
    final decoration =
        tester
                .widget<AnimatedContainer>(
                  find.descendant(
                    of: find.byType(MorphTabChip),
                    matching: find.byType(AnimatedContainer),
                  ),
                )
                .decoration!
            as BoxDecoration;
    expect(
      decoration.borderRadius,
      const BorderRadius.only(
        bottomLeft: Radius.circular(8),
        bottomRight: Radius.circular(8),
      ),
    );
    expect(decoration.border!.top.width, 2);
    expect(decoration.border!.bottom.width, 0);
  });

  testWidgets('left: nav full width at top, vertical list beside content', (
    tester,
  ) async {
    _desktopSize(tester);
    final tabs = MorphTabSet()..update(1, title: 'One');
    await tester.pumpWidget(
      _harness(tabs: tabs, position: MorphTabPosition.left),
    );

    final nav = tester.getRect(find.byKey(const Key('nav-row')));
    final list = tester.getRect(find.byKey(const Key('tab-list')));
    final content = tester.getRect(find.byKey(const Key('web-content')));
    final address = tester.getRect(find.byKey(const Key('address-bar')));

    expect(find.byKey(const Key('tab-strip')), findsNothing);
    expect(find.byType(MorphTabChip), findsNothing);
    expect(find.byType(MorphTabListTile), findsOneWidget);
    expect(nav.bottom, lessThanOrEqualTo(list.top + 0.5));
    expect(nav.bottom, lessThanOrEqualTo(content.top + 0.5));
    expect(list.right, lessThanOrEqualTo(content.left + 0.5));
    expect(address.width, greaterThan(400));
    expect(find.text('One'), findsOneWidget);
    expect(
      tester.getRect(find.byKey(const Key('window-close'))).bottom,
      lessThanOrEqualTo(nav.top + 0.5),
    );
    expect(find.byKey(const Key('tab-list-collapse')), findsOneWidget);
  });

  testWidgets('right: nav at top, list to the right of content', (
    tester,
  ) async {
    _desktopSize(tester);
    final tabs = MorphTabSet()..update(1, title: 'One');
    await tester.pumpWidget(
      _harness(tabs: tabs, position: MorphTabPosition.right),
    );

    final nav = tester.getRect(find.byKey(const Key('nav-row')));
    final list = tester.getRect(find.byKey(const Key('tab-list')));
    final content = tester.getRect(find.byKey(const Key('web-content')));

    expect(find.byType(MorphTabChip), findsNothing);
    expect(find.byType(MorphTabListTile), findsOneWidget);
    expect(nav.bottom, lessThanOrEqualTo(content.top + 0.5));
    expect(content.right, lessThanOrEqualTo(list.left + 0.5));
    expect(
      tester.getRect(find.byKey(const Key('window-close'))).bottom,
      lessThanOrEqualTo(nav.top + 0.5),
    );
  });

  testWidgets('macOS top strip pads leading inset so tabs start after lights', (
    tester,
  ) async {
    _desktopSize(tester);
    final tabs = MorphTabSet();
    await tester.pumpWidget(
      _harness(
        tabs: tabs,
        position: MorphTabPosition.top,
        platform: TargetPlatform.macOS,
      ),
    );

    final inset = tester.getSize(find.byKey(const Key('chrome-leading-inset')));
    expect(inset.width, MorphWindowChrome.macTrafficLightFallbackInset);
    final mark = tester.getRect(find.byKey(const Key('morph-mark')));
    final strip = tester.getRect(find.byKey(const Key('tab-strip')));
    expect(mark.left, greaterThanOrEqualTo(strip.left + 70));
    expect(mark.center.dy, closeTo(strip.center.dy, 2));
    expect(find.byKey(const Key('window-close')), findsNothing);
  });

  testWidgets('macOS caption bar keeps traffic-light padding when tabs move', (
    tester,
  ) async {
    _desktopSize(tester);
    final tabs = MorphTabSet();
    await tester.pumpWidget(
      _harness(
        tabs: tabs,
        position: MorphTabPosition.left,
        platform: TargetPlatform.macOS,
      ),
    );

    final inset = tester.getSize(find.byKey(const Key('chrome-leading-inset')));
    expect(inset.width, MorphWindowChrome.macTrafficLightFallbackInset);
    final mark = tester.getRect(find.byKey(const Key('morph-mark')));
    final caption = tester.getRect(find.byKey(const Key('caption-bar')));
    expect(mark.left, greaterThanOrEqualTo(caption.left + 70));
    expect(find.byKey(const Key('window-close')), findsNothing);
  });

  testWidgets('Hyprland close-only hides min/max on the top chrome', (
    tester,
  ) async {
    _desktopSize(tester);
    final tabs = MorphTabSet();
    await tester.pumpWidget(
      _harness(
        tabs: tabs,
        position: MorphTabPosition.bottom,
        mode: MorphWindowButtonMode.closeOnly,
      ),
    );
    expect(find.byKey(const Key('window-close')), findsOneWidget);
    expect(find.byKey(const Key('window-minimize')), findsNothing);
    expect(find.byKey(const Key('window-maximize')), findsNothing);
    expect(
      tester.getRect(find.byKey(const Key('window-close'))).bottom,
      lessThanOrEqualTo(
        tester.getRect(find.byKey(const Key('nav-row'))).top + 0.5,
      ),
    );
  });

  testWidgets('position button can move tabs to the left', (tester) async {
    _desktopSize(tester);
    final tabs = MorphTabSet();
    MorphTabPosition? chosen;
    await tester.pumpWidget(
      _harness(
        tabs: tabs,
        position: MorphTabPosition.top,
        onPositionChanged: (value) => chosen = value,
      ),
    );
    await tester.tap(find.byKey(const Key('tab-position')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('tab-position-left')));
    await tester.pumpAndSettle();
    expect(chosen, MorphTabPosition.left);
  });
}
