import 'package:jaspr/dom.dart';
import 'package:jaspr/jaspr.dart';

import '../components/layout.dart';
import '../content/features.dart';
import '../site.dart';

class FeaturesPage extends StatelessComponent {
  const FeaturesPage({super.key});

  @override
  Component build(BuildContext context) {
    final shipped = features.where((f) => f.status == Status.shipped);
    final next = features.where((f) => f.status != Status.shipped);
    return Component.fragment([
      pageHero(
        eyebrow: 'Features',
        title: 'What Morph does',
        lede:
            'What ships in Morph $latestVersion today, and what is coming. Screenshots are of the released app with a '
            'demo profile; planned features are shown as labeled concept illustrations.',
      ),
      section([
        div([
          h2([Component.text('In Morph $latestVersion')]),
          div([for (final f in shipped) _card(f)], classes: 'feature-list'),
        ], classes: 'wrap'),
      ], classes: 'band'),
      section([
        div([
          h2([Component.text('Coming soon')]),
          p([
            Component.text('Planned in the project priorities. See the '),
            a([Component.text('roadmap')], href: '/roadmap'),
            Component.text(' for when.'),
          ], classes: 'intro'),
          div([for (final f in next) _card(f)], classes: 'feature-list'),
        ], classes: 'wrap'),
      ], classes: 'band'),
    ]);
  }
}

Component _card(Feature f) {
  return article(
    [
      shot(f.image, f.alt, f.caption, concept: f.concept, lazy: false),
      div([
        h3([Component.text(f.title)]),
        p([
          statusTag(f.status),
          if (f.when.isNotEmpty)
            span([Component.text(f.status == Status.shipped ? 'since ${f.when}' : f.when)], classes: 'when'),
        ], classes: 'meta'),
        p([Component.text(f.body)]),
      ], classes: 'feature-text'),
    ],
    classes: 'card feature',
    id: f.id,
  );
}
