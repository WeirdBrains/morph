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
  bool sidebarCollapsed = false,
  ValueChanged<MorphTabPosition>? onPositionChanged,
  VoidCallback? onToggleSidebar,
  ValueChanged<int>? onSelectTab,
  ValueChanged<int>? onCloseTab,
  double sidebarWidth = 220,
  ValueChanged<double>? onSidebarWidthChanged,
  VoidCallback? onSidebarWidthCommit,
  VoidCallback? onSidebarWidthReset,
}) {
  return MaterialApp(
    theme: MorphTheme.dark,
    home: Scaffold(
      body: MorphDesktopChrome(
        tabs: tabs.tabs,
        activeId: tabs.activeId,
        position: position,
        sidebarCollapsed: sidebarCollapsed,
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
        onToggleSidebar: onToggleSidebar,
        sidebarWidth: sidebarWidth,
        onSidebarWidthChanged: onSidebarWidthChanged,
        onSidebarWidthCommit: onSidebarWidthCommit,
        onSidebarWidthReset: onSidebarWidthReset,
        onSelectTab: onSelectTab,
        onCloseTab: onCloseTab,
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

MorphTabSet _nTabs(int count) {
  final tabs = MorphTabSet()..update(1, title: 'Tab 1');
  for (var i = 2; i <= count; i++) {
    final id = tabs.open();
    tabs.update(id, title: 'Tab $i');
  }
  tabs.switchTo(1);
  return tabs;
}

Rect _railRect(WidgetTester tester, MorphTabPosition position) {
  final horizontal =
      position == MorphTabPosition.top || position == MorphTabPosition.bottom;
  return tester.getRect(find.byKey(Key(horizontal ? 'tab-strip' : 'tab-list')));
}

List<int> _visibleTabIds(
  WidgetTester tester,
  MorphTabSet tabs,
  MorphTabPosition position,
) {
  final rail = _railRect(tester, position).inflate(0.5);
  final ids = <int>[];
  for (final tab in tabs.tabs) {
    final finder = find.byKey(Key('tab-${tab.id}'));
    if (finder.evaluate().isEmpty) {
      continue;
    }
    if (rail.contains(tester.getRect(finder).center)) {
      ids.add(tab.id);
    }
  }
  return ids;
}

void _assertTrailingAfterLastTab(
  WidgetTester tester, {
  required MorphTabSet tabs,
  required MorphTabPosition position,
}) {
  final newBtn = tester.getRect(find.byKey(const Key('tab-new')));
  final posBtn = tester.getRect(find.byKey(const Key('tab-position')));
  final horizontal =
      position == MorphTabPosition.top || position == MorphTabPosition.bottom;
  final rail = _railRect(tester, position);
  expect(
    rail.inflate(0.5).contains(newBtn.center),
    isTrue,
    reason: '$position',
  );
  expect(
    rail.inflate(0.5).contains(posBtn.center),
    isTrue,
    reason: '$position',
  );
  final overflowFinder = find.byKey(const Key('tab-overflow'));
  final overflow = overflowFinder.evaluate().isNotEmpty;
  final visibleIds = _visibleTabIds(tester, tabs, position);
  expect(visibleIds, isNotEmpty, reason: '$position');
  final lastVisible = tester.getRect(find.byKey(Key('tab-${visibleIds.last}')));
  if (overflow) {
    expect(visibleIds.length, lessThan(tabs.length), reason: '$position');
    expect(
      find.byKey(Key('tab-${tabs.tabs.last.id}')),
      findsNothing,
      reason: '$position last tab belongs in the overflow menu',
    );
  } else {
    expect(visibleIds.length, tabs.length, reason: '$position');
  }
  if (horizontal) {
    expect(
      posBtn.left,
      greaterThanOrEqualTo(newBtn.right - 0.5),
      reason: '$position',
    );
    if (overflow) {
      final overflowBtn = tester.getRect(overflowFinder);
      expect(
        overflowBtn.left,
        greaterThanOrEqualTo(lastVisible.right - 0.5),
        reason: '$position overflow after last visible',
      );
      expect(
        newBtn.left,
        greaterThanOrEqualTo(overflowBtn.right - 1.5),
        reason: '$position + after overflow',
      );
      expect(overflowBtn.overlaps(lastVisible), isFalse, reason: '$position');
      expect(newBtn.overlaps(overflowBtn), isFalse, reason: '$position');
    } else {
      expect(
        newBtn.left,
        greaterThanOrEqualTo(lastVisible.right - 0.5),
        reason: '$position trailing',
      );
      expect(
        newBtn.overlaps(lastVisible),
        isFalse,
        reason: '$position overlap',
      );
      expect(
        newBtn.left - lastVisible.right,
        lessThan(16),
        reason: '$position gap',
      );
    }
  } else {
    expect(
      posBtn.top,
      greaterThanOrEqualTo(newBtn.bottom - 0.5),
      reason: '$position',
    );
    if (overflow) {
      final overflowBtn = tester.getRect(overflowFinder);
      expect(
        overflowBtn.top,
        greaterThanOrEqualTo(lastVisible.bottom - 0.5),
        reason: '$position overflow below last visible',
      );
      expect(
        newBtn.top,
        greaterThanOrEqualTo(overflowBtn.bottom - 1.5),
        reason: '$position + below overflow',
      );
    } else {
      expect(
        newBtn.top,
        greaterThanOrEqualTo(lastVisible.bottom - 0.5),
        reason: '$position',
      );
      expect(
        newBtn.overlaps(lastVisible),
        isFalse,
        reason: '$position overlap',
      );
      expect(
        newBtn.top - lastVisible.bottom,
        lessThan(16),
        reason: '$position gap',
      );
    }
  }
}

void _assertWindowButtonsOnScreen(
  WidgetTester tester, {
  required MorphWindowButtonMode mode,
  required Size size,
  required MorphTabPosition position,
}) {
  final chrome = tester.getRect(find.byKey(const Key('desktop-chrome')));
  void onScreen(Key key) {
    expect(
      find.byKey(key),
      findsOneWidget,
      reason: '$key $position $size $mode',
    );
    final rect = tester.getRect(find.byKey(key));
    expect(
      rect.left,
      greaterThanOrEqualTo(chrome.left - 0.5),
      reason: '$key $position $size',
    );
    expect(
      rect.right,
      lessThanOrEqualTo(chrome.right + 0.5),
      reason: '$key $position $size',
    );
    expect(
      rect.top,
      greaterThanOrEqualTo(chrome.top - 0.5),
      reason: '$key $position $size',
    );
    expect(
      rect.bottom,
      lessThanOrEqualTo(chrome.bottom + 0.5),
      reason: '$key $position $size',
    );
  }

  onScreen(const Key('window-close'));
  if (mode == MorphWindowButtonMode.all) {
    onScreen(const Key('window-minimize'));
    onScreen(const Key('window-maximize'));
  } else {
    expect(find.byKey(const Key('window-minimize')), findsNothing);
    expect(find.byKey(const Key('window-maximize')), findsNothing);
  }
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
    final tab = tester.getRect(find.byType(MorphTabChip));
    final indicator = tester.getRect(find.byKey(const Key('tab-indicator-1')));
    expect(indicator.left, closeTo(tab.left, 0.5));
    expect(indicator.right, closeTo(tab.right, 0.5));
    expect(indicator.bottom, closeTo(tab.bottom, 0.5));
    expect(indicator.height, closeTo(2, 0.5));
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
    final tab = tester.getRect(find.byType(MorphTabChip));
    final indicator = tester.getRect(find.byKey(const Key('tab-indicator-1')));
    expect(indicator.left, closeTo(tab.left, 0.5));
    expect(indicator.right, closeTo(tab.right, 0.5));
    expect(indicator.top, closeTo(tab.top, 0.5));
    expect(indicator.height, closeTo(2, 0.5));
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

  testWidgets('left and right: collapse then expand round trip', (
    tester,
  ) async {
    _desktopSize(tester);
    final tabs = MorphTabSet()..update(1, title: 'One');
    for (final position in [MorphTabPosition.left, MorphTabPosition.right]) {
      var collapsed = false;
      await tester.pumpWidget(
        MaterialApp(
          theme: MorphTheme.dark,
          home: Scaffold(
            body: StatefulBuilder(
              builder: (context, setState) {
                return MorphDesktopChrome(
                  tabs: tabs.tabs,
                  activeId: tabs.activeId,
                  position: position,
                  sidebarCollapsed: collapsed,
                  platform: TargetPlatform.linux,
                  navigation: const SizedBox(
                    key: Key('nav-row'),
                    height: 48,
                    child: TextField(key: Key('address-bar')),
                  ),
                  content: const ColoredBox(
                    key: Key('web-content'),
                    color: Colors.black,
                    child: SizedBox.expand(),
                  ),
                  onToggleSidebar: () => setState(() => collapsed = !collapsed),
                );
              },
            ),
          ),
        ),
      );

      expect(
        tester.getSize(find.byKey(const Key('tab-list'))).width,
        MorphTabList.expandedWidth,
        reason: '$position expanded',
      );
      expect(find.text('One'), findsOneWidget);
      expect(find.byKey(const Key('tab-list-collapse')), findsOneWidget);

      await tester.tap(find.byKey(const Key('tab-list-collapse')));
      await tester.pumpAndSettle();

      expect(
        tester.getSize(find.byKey(const Key('tab-list'))).width,
        MorphTabList.collapsedWidth,
        reason: '$position collapsed',
      );
      expect(find.text('One'), findsNothing);
      expect(find.byKey(const Key('tab-list-collapse')), findsOneWidget);
      expect(find.byKey(const Key('tab-new')), findsOneWidget);
      expect(find.byKey(const Key('tab-position')), findsOneWidget);
      // The expand control stays inside the rail and is tappable.
      final list = tester.getRect(find.byKey(const Key('tab-list')));
      final toggle = tester.getRect(find.byKey(const Key('tab-list-collapse')));
      expect(
        list.inflate(0.5).contains(toggle.center),
        isTrue,
        reason: '$position',
      );
      // Expand stays at the top; + and position trail the last tab.
      final lastTab = tester.getRect(find.byKey(const Key('tab-1')));
      final newBtn = tester.getRect(find.byKey(const Key('tab-new')));
      final posBtn = tester.getRect(find.byKey(const Key('tab-position')));
      expect(
        toggle.bottom,
        lessThanOrEqualTo(lastTab.top + 0.5),
        reason: '$position',
      );
      expect(
        newBtn.top,
        greaterThanOrEqualTo(lastTab.bottom - 0.5),
        reason: '$position',
      );
      expect(
        posBtn.top,
        greaterThanOrEqualTo(newBtn.bottom - 0.5),
        reason: '$position',
      );

      await tester.tap(find.byKey(const Key('tab-list-collapse')));
      await tester.pumpAndSettle();

      expect(
        tester.getSize(find.byKey(const Key('tab-list'))).width,
        MorphTabList.expandedWidth,
        reason: '$position re-expanded',
      );
      expect(find.text('One'), findsOneWidget);
    }
  });

  testWidgets('collapsed rails do not overflow at 48px or in a narrow window', (
    tester,
  ) async {
    final overflows = <String>[];
    final previous = FlutterError.onError;
    FlutterError.onError = (details) {
      final text = '${details.exceptionAsString()}\n${details.toString()}';
      if (text.contains('overflowed') || text.contains('OVERFLOWING')) {
        overflows.add(text);
      }
      previous?.call(details);
    };
    addTearDown(() => FlutterError.onError = previous);

    final tabs = MorphTabSet()..update(1, title: 'One');
    for (final size in const [Size(1280, 800), Size(580, 800)]) {
      final view = tester.view;
      view.physicalSize = size;
      view.devicePixelRatio = 1.0;
      addTearDown(view.reset);
      for (final position in [MorphTabPosition.left, MorphTabPosition.right]) {
        await tester.pumpWidget(
          _harness(tabs: tabs, position: position, sidebarCollapsed: true),
        );
        await tester.pump();
        expect(
          tester.getSize(find.byKey(const Key('tab-list'))).width,
          MorphTabList.collapsedWidth,
          reason: '$position at ${size.width}',
        );
        expect(find.byKey(const Key('tab-list-collapse')), findsOneWidget);
        final list = tester.getRect(find.byKey(const Key('tab-list')));
        final toggle = tester.getRect(
          find.byKey(const Key('tab-list-collapse')),
        );
        expect(
          list.inflate(0.5).contains(toggle.center),
          isTrue,
          reason: '$position at ${size.width}',
        );
      }
    }
    expect(tester.takeException(), isNull);
    expect(overflows, isEmpty);
  });

  testWidgets('trailing + and position follow the last tab in every layout', (
    tester,
  ) async {
    for (final count in [1, 3, 20]) {
      for (final position in MorphTabPosition.values) {
        _desktopSize(tester);
        final tabs = _nTabs(count);
        await tester.pumpWidget(_harness(tabs: tabs, position: position));
        await tester.pumpAndSettle();
        _assertTrailingAfterLastTab(tester, tabs: tabs, position: position);
      }
    }
  });

  testWidgets('window buttons stay on screen in every layout and width', (
    tester,
  ) async {
    final overflows = <String>[];
    final previous = FlutterError.onError;
    FlutterError.onError = (details) {
      final text = '${details.exceptionAsString()}\n${details.toString()}';
      if (text.contains('overflowed') || text.contains('OVERFLOWING')) {
        overflows.add(text);
      }
      previous?.call(details);
    };
    addTearDown(() => FlutterError.onError = previous);

    final tabs = _nTabs(3);
    for (final size in const [Size(1280, 800), Size(580, 800)]) {
      final view = tester.view;
      view.physicalSize = size;
      view.devicePixelRatio = 1.0;
      addTearDown(view.reset);
      for (final position in MorphTabPosition.values) {
        for (final mode in MorphWindowButtonMode.values) {
          await tester.pumpWidget(
            _harness(tabs: tabs, position: position, mode: mode),
          );
          await tester.pumpAndSettle();
          _assertWindowButtonsOnScreen(
            tester,
            mode: mode,
            size: size,
            position: position,
          );
        }
      }
    }
    expect(tester.takeException(), isNull);
    expect(overflows, isEmpty);
  });
  testWidgets('overflow menu lists hidden tabs and swaps the active tab in', (
    tester,
  ) async {
    _desktopSize(tester);
    for (final position in MorphTabPosition.values) {
      var tabs = _nTabs(20);
      late void Function(void Function()) setHarness;
      await tester.pumpWidget(
        StatefulBuilder(
          builder: (context, setState) {
            setHarness = setState;
            return _harness(
              tabs: tabs,
              position: position,
              onSelectTab: (id) {
                setState(() => tabs.switchTo(id));
              },
              onCloseTab: (id) {
                setState(() => tabs.close(id));
              },
            );
          },
        ),
      );
      await tester.pumpAndSettle();

      final before = _visibleTabIds(tester, tabs, position);
      expect(
        find.byKey(const Key('tab-overflow')),
        findsOneWidget,
        reason: '$position',
      );
      expect(before, isNot(contains(tabs.tabs.last.id)), reason: '$position');
      expect(find.byKey(Key('tab-${tabs.tabs.last.id}')), findsNothing);

      await tester.tap(find.byKey(const Key('tab-overflow')));
      await tester.pumpAndSettle();
      final hiddenId = tabs.tabs.last.id;
      expect(find.byKey(Key('tab-overflow-$hiddenId')), findsOneWidget);
      expect(find.byKey(Key('tab-overflow-close-$hiddenId')), findsOneWidget);

      await tester.tap(find.byKey(Key('tab-overflow-$hiddenId')));
      await tester.pumpAndSettle();
      expect(tabs.activeId, hiddenId, reason: '$position');
      expect(
        find.byKey(Key('tab-$hiddenId')),
        findsOneWidget,
        reason: '$position active tab swapped into the strip',
      );
      final after = _visibleTabIds(tester, tabs, position);
      expect(after, contains(hiddenId), reason: '$position');
      expect(after, isNot(contains(before.first)), reason: '$position');

      await tester.tap(find.byKey(const Key('tab-overflow')));
      await tester.pumpAndSettle();
      final closeId = tabs.tabs.first.id;
      expect(find.byKey(Key('tab-overflow-close-$closeId')), findsOneWidget);
      await tester.tap(find.byKey(Key('tab-overflow-close-$closeId')));
      await tester.pumpAndSettle();
      expect(tabs.tab(closeId), isNull, reason: '$position');
      expect(find.byKey(Key('tab-overflow-$closeId')), findsNothing);
      setHarness(() {});
    }
  });
  testWidgets('left and right sidebars drag, clamp, reset, and restore width', (
    tester,
  ) async {
    for (final position in [MorphTabPosition.left, MorphTabPosition.right]) {
      _desktopSize(tester);
      var width = 220.0;
      var committed = <double>[];
      late void Function(void Function()) setHarness;
      var collapsed = false;
      final tabs = _nTabs(3);
      await tester.pumpWidget(
        StatefulBuilder(
          builder: (context, setState) {
            setHarness = setState;
            return _harness(
              tabs: tabs,
              position: position,
              sidebarCollapsed: collapsed,
              sidebarWidth: width,
              onSidebarWidthChanged: (next) {
                setState(() => width = next);
              },
              onSidebarWidthCommit: () => committed.add(width),
              onSidebarWidthReset: () {
                setState(() => width = 220);
                committed.add(220);
              },
              onToggleSidebar: () {
                setState(() => collapsed = !collapsed);
              },
            );
          },
        ),
      );
      await tester.pumpAndSettle();

      expect(
        tester.getSize(find.byKey(const Key('tab-list'))).width,
        220,
        reason: '$position default',
      );
      final cursor = tester
          .widget<MouseRegion>(
            find.ancestor(
              of: find.byKey(const Key('sidebar-resize')),
              matching: find.byType(MouseRegion),
            ),
          )
          .cursor;
      expect(cursor, SystemMouseCursors.resizeColumn, reason: '$position');

      final outward = position == MorphTabPosition.left
          ? const Offset(800, 0)
          : const Offset(-800, 0);
      await tester.drag(find.byKey(const Key('sidebar-resize')), outward);
      await tester.pumpAndSettle();
      expect(width, 480, reason: '$position max clamp');
      expect(
        tester.getSize(find.byKey(const Key('tab-list'))).width,
        480,
        reason: '$position max size',
      );
      expect(committed, isNotEmpty, reason: '$position commit');

      final inward = position == MorphTabPosition.left
          ? const Offset(-800, 0)
          : const Offset(800, 0);
      await tester.drag(find.byKey(const Key('sidebar-resize')), inward);
      await tester.pumpAndSettle();
      expect(width, 160, reason: '$position min clamp');

      await tester.tap(find.byKey(const Key('sidebar-resize')));
      await tester.pump(const Duration(milliseconds: 50));
      await tester.tap(find.byKey(const Key('sidebar-resize')));
      await tester.pumpAndSettle();
      expect(width, 220, reason: '$position double-click reset');

      setHarness(() => width = 300);
      await tester.pumpAndSettle();
      expect(tester.getSize(find.byKey(const Key('tab-list'))).width, 300);
      await tester.tap(find.byKey(const Key('tab-list-collapse')));
      await tester.pumpAndSettle();
      expect(
        tester.getSize(find.byKey(const Key('tab-list'))).width,
        48,
        reason: '$position collapsed rail',
      );
      expect(find.byKey(const Key('sidebar-resize')), findsNothing);
      await tester.tap(find.byKey(const Key('tab-list-collapse')));
      await tester.pumpAndSettle();
      expect(
        tester.getSize(find.byKey(const Key('tab-list'))).width,
        300,
        reason: '$position expand restores saved width',
      );
    }

    // Narrow window: 480 is not allowed when it would eat the content.
    final view = tester.view;
    view.physicalSize = const Size(500, 800);
    view.devicePixelRatio = 1.0;
    addTearDown(view.reset);
    var width = 220.0;
    await tester.pumpWidget(
      StatefulBuilder(
        builder: (context, setState) {
          return _harness(
            tabs: _nTabs(3),
            position: MorphTabPosition.left,
            sidebarWidth: width,
            onSidebarWidthChanged: (next) {
              setState(() => width = next);
            },
          );
        },
      ),
    );
    await tester.pumpAndSettle();
    await tester.drag(
      find.byKey(const Key('sidebar-resize')),
      const Offset(800, 0),
    );
    await tester.pumpAndSettle();
    expect(width, 252, reason: 'narrow window clamp (500-48-200 = 252)');
  });
}
