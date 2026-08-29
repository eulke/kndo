# `kndo:thymeleaf`

**Status:** Shipped, built-in. Activation: any manifest declares `spring-boot-starter-thymeleaf` ·
**Implements:** `contribute_roots`, `contribute_edges` ·
**Crate:** `crates/kndo-plugin-thymeleaf`

## The gap

A Spring Boot app's view layer is **two hops** the language graph cannot see, and neither is
an import:

```java
@GetMapping("/")
String welcome() {
    return "welcome";            // hop 1: a logical view name, as a bare string
}
```

```html
<link rel="stylesheet" th:href="@{/resources/css/petclinic.css}" />
<!-- hop 2: a URL that becomes a file only through Spring Boot's static-resource locations -->
```

Spring's view resolver turns `"welcome"` into `classpath:/templates/welcome.html`; the
template then names a stylesheet by URL. Without the plugin the whole view layer — every
template, and every asset any of them links — reads as unreachable. In spring-petclinic that
is `petclinic.css`, and it is the reason this plugin exists.

## Why the starter, not `thymeleaf`

What this plugin knows is a **pairing**: Thymeleaf's link expression *plus* Spring Boot's
template prefix and static locations. Thymeleaf alone answers neither hop. The
`spring-boot-starter-thymeleaf` artifact is the one coordinate that means both are in play.
Matching an artifact id against a `groupId:artifactId` coordinate is the JVM adapters' own
answer (`LanguageAdapter::declares_dependency`), so the gate costs no new mechanism.

## The rules

1. **Every file under the template prefix is a Production root.** Deliberately over-rooting,
   in the only direction a root can err safely: Spring's resolver can load *any*
   file under `templates/` by a logical name a controller produces as a bare string, and no
   plugin can follow that string. A template nothing names stays alive; the alternative —
   rooting none of them — kills every asset the view layer links. It costs nothing directly
   either way, since no adapter claims `.html` and an unclaimed file is never reported
   `unused`; what the root buys is a live `from` for rule 2.
2. **Each link attribute's URL becomes a file-liveness edge**, resolved through Spring Boot's
   static locations — `META-INF/resources`, `resources`, `static`, `public`, in
   `WebProperties.Resources.CLASSPATH_RESOURCE_LOCATIONS`'s own order — and only where a file
   is actually there. `Certain`, because the attribute is literal and the path it names
   exists; an unresolvable URL never becomes an edge at all.
3. **Both the `th:` forms and the plain ones** (`th:href`/`th:src`, `href`/`src`). The plain
   forms are the design-time fallbacks Thymeleaf's natural-templating stance encourages
   (`src="../static/…"`, so the file opens unrendered in a browser) — a reference a human
   wrote and can break, and one that costs nothing to follow because an unresolvable value
   contributes nothing.

## Proven by

`thymeleaf_link_expression_is_gated_by_the_starter` in
`crates/kndo/tests/builtin_plugin_proofs.rs` — spring-petclinic reduced to controller,
template, stylesheet and the Sass it is compiled from. Without the starter declared the CSS is
reported `unused`; with it declared the finding goes and the fixture's unrelated dead files
stay reported. The contribution record is asserted at one root and one edge.

Note the fixture deliberately declares **no** `libsass-maven-plugin` in either run of that
test: that plugin also removes the CSS finding, by a different mechanism, and a baseline where
two mechanisms produce the same delta proves neither.

## What it does not do

- **It does not read `spring.thymeleaf.prefix`/`suffix`.** The default `templates/` is what is
  implemented; a project that moves the prefix gets silence, not a guess.
- **It does not follow the controller's string to its template.** Rule 1 exists precisely
  because that hop is unfollowable — the string is often assembled, and the resolver's
  behavior depends on runtime configuration.
- **It parses no Java and no Thymeleaf expression language.** A `@{…}` whose body is not a
  literal path resolves to nothing.
- **No JSP, no FreeMarker, no Mustache.** Sibling view technologies with their own resolvers
  are their own plugins, each needing its own measured case.
