//! `kndo:thymeleaf` — the built-in Spring/Thymeleaf view-layer plugin.
//!
//! A Spring Boot app's view layer is two hops the language graph cannot see. A controller
//! returns the *logical view name* `"welcome"` as a bare string and Spring's view resolver
//! turns it into `classpath:/templates/welcome.html`; that template then links its stylesheet
//! with `th:href="@{/resources/css/petclinic.css}"`, a URL that only becomes a file through
//! Spring Boot's static-resource locations. Neither hop is an import, so the whole view layer
//! — every template and every asset it links — reads as unreachable. In spring-petclinic that
//! is `petclinic.css`, and it is the whole reason this plugin exists.
//!
//! The knowledge here is Thymeleaf's link expression and Spring Boot's two location
//! conventions, and nothing else: it reads no source, parses no grammar, and resolves nothing
//! it cannot point at an actual file for.

use kndo_core::adapter::ProjectPath;
use kndo_core::plugin::{
    ActivationRule, ContentView, EdgeSink, GraphView, Plugin, PluginDescriptor, PluginTarget,
    RootSink,
};
use kndo_core::vocab::{Confidence, RefKind, RootKind};
use rustc_hash::FxHashSet;
use smol_str::SmolStr;

pub struct ThymeleafPlugin;

/// Spring Boot's static-resource locations, relative to the classpath root
/// (`WebProperties.Resources.CLASSPATH_RESOURCE_LOCATIONS`), in its own order. A URL served by
/// the app resolves against these and nothing else.
const STATIC_LOCATIONS: &[&str] = &["META-INF/resources", "resources", "static", "public"];

/// The attributes whose value is a URL the rendered page fetches.
///
/// The `th:` forms are what Thymeleaf evaluates. The plain forms are the *design-time*
/// fallbacks Thymeleaf's natural-templating stance encourages (`src="../static/…"`, so the
/// file opens in a browser unrendered) — a reference a human wrote and can break, and one that
/// costs nothing to follow because an unresolvable value contributes nothing.
const LINK_ATTRIBUTES: &[&str] = &["th:href", "th:src", "href", "src"];

/// The template directory `spring.thymeleaf.prefix` names by default.
const TEMPLATE_DIR: &str = "templates/";

impl Plugin for ThymeleafPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: SmolStr::new("kndo:thymeleaf"),
            version: SmolStr::new("1"),
            // Empty: the gate below IS a rule, so prose beside it would be the same fact twice.
            detection: vec![],
            requested_file_access: vec![SmolStr::new("**/templates/**/*.html")],
            // The starter, not `thymeleaf` itself: what this plugin knows is the PAIRING —
            // Thymeleaf's link expression plus Spring Boot's template prefix and static
            // locations — and the starter is the one artifact that means both are in play.
            // Matching an artifact id against a `groupId:artifactId` coordinate is the JVM
            // adapters' own answer (`LanguageAdapter::declares_dependency`).
            activation: vec![ActivationRule::ManifestDependency(SmolStr::new(
                "spring-boot-starter-thymeleaf",
            ))],
            dependencies: vec![],
        }
    }

    fn mutates_graph(&self) -> bool {
        true
    }

    /// Every template is a root, because Spring's view resolver can load ANY file under the
    /// template prefix by logical name and the name is produced by a controller returning a
    /// bare string — which no plugin can follow to its template.
    ///
    /// Deliberately over-rooting, in the only direction a root can err safely: a template no
    /// controller ever names stays alive, where the alternative — rooting none of them — kills
    /// every asset the whole view layer links. It costs nothing today either way,
    /// since no adapter claims `.html` and an unclaimed file is never reported `unused`; what
    /// the root buys is a live `from` for the edges below.
    fn contribute_roots(
        &self,
        _graph: &GraphView<'_>,
        content: &ContentView<'_>,
        out: &mut RootSink,
    ) {
        for path in content.matching_paths() {
            out.add(
                PluginTarget::file(path.clone()),
                RootKind::Production,
                Confidence::Certain,
            );
        }
    }

    fn contribute_edges(
        &self,
        graph: &GraphView<'_>,
        content: &ContentView<'_>,
        out: &mut EdgeSink,
    ) {
        let files: FxHashSet<&str> = graph.files().map(|f| f.path.0.as_str()).collect();
        for template in content.matching_paths() {
            let Some(bytes) = content.read(template) else {
                continue;
            };
            let Ok(text) = std::str::from_utf8(&bytes) else {
                continue; // not text: nothing to read, contribute nothing
            };
            for value in link_values(text) {
                let Some(url) = url_path(value) else { continue };
                for target in resolve(&url, template.0.as_str(), &files) {
                    // A file-target `to` is the file-liveness edge (`ReferencesFile`), where
                    // `kind` is ignored — liveness evidence only, which is all a template link
                    // is. `Certain`: the attribute is literal in the file and the path it
                    // names is a file that exists; an unresolvable one never gets here.
                    out.add(
                        PluginTarget::file(template.clone()),
                        PluginTarget::file(ProjectPath(SmolStr::new(target))),
                        RefKind::Read,
                        Confidence::Certain,
                    );
                }
            }
        }
    }
}

/// Every link attribute's raw value, in document order.
///
/// A scanner rather than an HTML parser, deliberately: templates are HTML5 with unclosed void
/// elements that no XML parser accepts, and the only thing needed here is an attribute value.
/// Matching requires the character before the name to be a delimiter, so `data-href=` and
/// `xlink:href=` are not mistaken for `href=`.
fn link_values(html: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for (start, _) in html.match_indices('=') {
        let before = &html[..start];
        let Some(name_start) = before.rfind(|c: char| c.is_whitespace() || c == '<') else {
            continue;
        };
        let name = before[name_start + 1..].trim();
        if !LINK_ATTRIBUTES.contains(&name) {
            continue;
        }
        let after = html[start + 1..].trim_start();
        let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            continue; // an unquoted attribute value: not worth guessing where it ends
        };
        let rest = &after[quote.len_utf8()..];
        let Some(end) = rest.find(quote) else {
            continue;
        };
        let offset = html.len() - rest.len();
        out.push(&html[offset..offset + end]);
    }
    out
}

/// The URL path an attribute value names, or `None` when it names something that is not a
/// static path.
///
/// `@{…}` is Thymeleaf's link expression; its content is a context-relative URL. Everything
/// else in that syntax — `${…}` variables, `+` concatenation, quoted fragments, the `__…__`
/// preprocessing form — builds a value only the running app knows, and a *route*
/// (`@{/owners/new}`) names no file either. Rather than enumerate those, this accepts only a
/// value that is entirely path characters, which excludes every one of them by construction.
fn url_path(value: &str) -> Option<String> {
    let value = value.trim();
    let inner = match value.strip_prefix("@{") {
        Some(rest) => rest.strip_suffix('}')?,
        None => value,
    };
    // A query string or fragment is not part of the file name.
    let path = inner.split(['?', '#']).next()?.trim();
    let plausible = !path.is_empty()
        && path != "/"
        && path
            .chars()
            .all(|c| c.is_alphanumeric() || "/._-~".contains(c));
    plausible.then(|| path.to_string())
}

/// The project files a template's URL can name — at most one, and none unless it exists.
///
/// A context-relative URL (`/resources/css/app.css`) is served from a static location under
/// the classpath root, and the classpath root is recoverable from the template's OWN path:
/// `src/main/resources/templates/fragments/layout.html` sits under `src/main/resources`, so the
/// candidates are `src/main/resources/<location>/resources/css/app.css`. Deriving the root from
/// the template rather than searching the whole project for a matching suffix is what keeps a
/// monorepo's second Spring app from claiming this one's assets.
///
/// Anything else is a design-time path relative to the template's own directory.
fn resolve(url: &str, template: &str, files: &FxHashSet<&str>) -> Vec<String> {
    let candidates: Vec<String> = match url.strip_prefix('/') {
        Some(served) => {
            let Some(classpath_root) = classpath_root(template) else {
                return Vec::new();
            };
            STATIC_LOCATIONS
                .iter()
                .map(|location| {
                    kndo_core::paths::join(classpath_root, &format!("{location}/{served}"))
                })
                .collect()
        }
        None => vec![kndo_core::paths::join(
            kndo_core::paths::dirname(template),
            url,
        )],
    };
    candidates
        .into_iter()
        .filter(|c| files.contains(c.as_str()))
        .collect()
}

/// The classpath root a template sits under: everything before its `templates/` segment.
/// `None` when the path has no such segment, which the content glob makes unreachable.
fn classpath_root(template: &str) -> Option<&str> {
    match template.find(TEMPLATE_DIR) {
        Some(0) => Some(""),
        Some(i) => Some(&template[..i - 1]),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_claims_the_reserved_namespace_and_gates_on_the_starter() {
        let d = ThymeleafPlugin.descriptor();
        assert_eq!(d.id, "kndo:thymeleaf");
        assert!(kndo_core::plugin::is_reserved_id(&d.id));
        assert_eq!(
            d.activation,
            vec![ActivationRule::ManifestDependency(SmolStr::new(
                "spring-boot-starter-thymeleaf"
            ))]
        );
        assert!(ThymeleafPlugin.mutates_graph());
    }

    #[test]
    fn the_petclinic_stylesheet_link_that_motivated_this_resolves() {
        // spring-petclinic's `fragments/layout.html`, verbatim.
        let html = r#"<link rel="stylesheet" th:href="@{/resources/css/petclinic.css}" />"#;
        let values = link_values(html);
        assert!(
            values.contains(&"@{/resources/css/petclinic.css}"),
            "{values:?}"
        );
        let url = url_path("@{/resources/css/petclinic.css}").unwrap();
        let files = FxHashSet::from_iter(["src/main/resources/static/resources/css/petclinic.css"]);
        assert_eq!(
            resolve(
                &url,
                "src/main/resources/templates/fragments/layout.html",
                &files
            ),
            vec!["src/main/resources/static/resources/css/petclinic.css"]
        );
    }

    #[test]
    fn a_dynamic_or_route_valued_link_names_no_file() {
        // Every one of these is real, from spring-petclinic's templates.
        for value in [
            "@{'/owners?page=' + ${i}}",
            "@{/owners/__${owner.id}__}",
            "@{__${link}__}",
            "@{'/vets.html?page=1'}",
        ] {
            assert!(url_path(value).is_none(), "{value}");
        }
        // A route IS a plausible path, and resolves to nothing because no file is there —
        // which is the safety property: resolution is evidence, never inference.
        let url = url_path("@{/owners/new}").unwrap();
        let files = FxHashSet::from_iter(["src/main/java/Owner.java"]);
        assert!(resolve(&url, "src/main/resources/templates/x.html", &files).is_empty());
    }

    #[test]
    fn a_design_time_relative_path_resolves_against_the_template() {
        let url = url_path("../static/resources/images/pets.png").unwrap();
        let files = FxHashSet::from_iter(["src/main/resources/static/resources/images/pets.png"]);
        assert_eq!(
            resolve(&url, "src/main/resources/templates/welcome.html", &files),
            vec!["src/main/resources/static/resources/images/pets.png"]
        );
        // …and petclinic's own stale one, pointing where no file is, contributes nothing.
        let stale = url_path("../static/images/spring-logo.svg").unwrap();
        assert!(resolve(&stale, "src/main/resources/templates/welcome.html", &files).is_empty());
    }

    #[test]
    fn an_attribute_whose_name_merely_ends_in_href_is_not_a_link() {
        let html = r#"<a data-href="@{/a.css}" xlink:href="@{/b.css}" href="@{/c.css}">x</a>"#;
        assert_eq!(link_values(html), vec!["@{/c.css}"]);
    }

    #[test]
    fn single_quotes_and_unquoted_values_are_handled() {
        assert_eq!(link_values("<img src='/a.png'>"), vec!["/a.png"]);
        assert!(link_values("<img src=/a.png>").is_empty());
    }

    #[test]
    fn the_classpath_root_comes_from_the_templates_segment() {
        assert_eq!(
            classpath_root("src/main/resources/templates/a.html"),
            Some("src/main/resources")
        );
        assert_eq!(classpath_root("templates/a.html"), Some(""));
        assert_eq!(classpath_root("src/a.html"), None);
    }

    #[test]
    fn every_static_location_spring_serves_is_tried() {
        let url = url_path("@{/css/app.css}").unwrap();
        for location in STATIC_LOCATIONS {
            let file = format!("app/{location}/css/app.css");
            let files = FxHashSet::from_iter([file.as_str()]);
            assert_eq!(
                resolve(&url, "app/templates/a.html", &files),
                vec![file.clone()],
                "{location}"
            );
        }
    }
}
