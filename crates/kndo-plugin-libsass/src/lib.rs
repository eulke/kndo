//! `kndo:libsass-maven-plugin` — the Maven Sass build's output, and where it comes from.
//!
//! A project can compile its stylesheets into its own source tree and commit the result.
//! spring-petclinic does: `com.gitlab.haynes:libsass-maven-plugin` reads `src/main/scss/` and
//! writes `src/main/resources/static/resources/css/petclinic.css`, and both sides are in git.
//!
//! kndo sees two unrelated things. The CSS carries no `@generated` marker (libsass emits none)
//! and sits under a perfectly ordinary path, so it reads as authored — and the moment anything
//! makes it reachable, every custom property the compiled Bootstrap bundle declares and never
//! reads is reported. Meanwhile `src/main/scss/` is referenced by nothing at all and reads as a
//! dead directory.
//!
//! The pom knows both facts. This plugin reads exactly the two configuration values that carry
//! them and nothing else: it parses no Sass, resolves no imports, and asserts nothing about a
//! path it cannot point at a real file for.

use kndo_core::adapter::ProjectPath;
use kndo_core::plugin::{
    ActivationRule, ContentView, EdgeSink, GraphView, Plugin, PluginDescriptor, PluginTarget,
};
use kndo_core::vocab::{Confidence, FileClass, FileOrigin, RefKind};
use smol_str::SmolStr;

pub struct LibsassMavenPlugin;

/// The Maven coordinate's artifact id — the whole of this plugin's tool knowledge, beside the
/// two configuration element names below.
const ARTIFACT_ID: &str = "libsass-maven-plugin";

impl Plugin for LibsassMavenPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: SmolStr::new("kndo:libsass-maven-plugin"),
            version: SmolStr::new("1"),
            // Empty: the gate below IS a rule, so prose beside it would be the same fact twice.
            detection: vec![],
            requested_file_access: vec![SmolStr::new("**/pom.xml")],
            // A Maven BUILD plugin lives in `<build><plugins>`, never in `<dependencies>`, so
            // no `ManifestDependency` rule can name it — and recording it as a dependency to
            // make one work would corrupt `undeclared`/`version-skew`, which are about what the
            // project links against. So this gates on the file, like `kndo:info-plist`, and
            // pays the glob. `*.scss` rather than `**/pom.xml`: with no Sass anywhere, a Sass
            // build plugin has nothing to say, and this is the narrower of the two signals.
            activation: vec![ActivationRule::FileExists(SmolStr::new("**/*.scss"))],
            dependencies: vec![],
        }
    }

    fn mutates_graph(&self) -> bool {
        true
    }

    /// Anything under a declared `outputPath` is build output. Deliberate and narrow: only the
    /// origin axis moves, `role` is left exactly as the language decided.
    fn classify_file(
        &self,
        path: &ProjectPath,
        current: FileClass,
        content: &ContentView<'_>,
    ) -> Option<FileClass> {
        let under_an_output = compilations(content)
            .iter()
            .any(|c| is_under(path.0.as_str(), &c.output));
        under_an_output.then_some(FileClass {
            role: current.role,
            origin: FileOrigin::Generated,
        })
    }

    /// Output → input. `ReferencesFile` reads "if `from` is alive, that file is in use", so the
    /// direction is: the stylesheet ships, therefore the Sass it was compiled from is in use.
    /// The reverse would say the opposite of what is true — the `.scss` is what nothing
    /// references.
    fn contribute_edges(
        &self,
        graph: &GraphView<'_>,
        content: &ContentView<'_>,
        out: &mut EdgeSink,
    ) {
        let paths: Vec<&str> = graph.files().map(|f| f.path.0.as_str()).collect();
        for compilation in compilations(content) {
            let outputs: Vec<&&str> = paths
                .iter()
                .filter(|p| is_under(p, &compilation.output))
                .collect();
            let inputs: Vec<&&str> = paths
                .iter()
                .filter(|p| is_under(p, &compilation.input) && p.ends_with(".scss"))
                .collect();
            for output in &outputs {
                for input in &inputs {
                    // `Certain`: the pom declares this compilation, and both ends are files
                    // that exist. A file-target `to` is the file-liveness edge, where `kind`
                    // is ignored.
                    out.add(
                        PluginTarget::file(ProjectPath(SmolStr::new(**output))),
                        PluginTarget::file(ProjectPath(SmolStr::new(**input))),
                        RefKind::Read,
                        Confidence::Certain,
                    );
                }
            }
        }
    }
}

/// One declared compilation: a directory of Sass in, a directory of CSS out. Project-relative.
#[derive(Debug, PartialEq)]
struct Compilation {
    input: String,
    output: String,
}

/// Every compilation the project's poms declare.
fn compilations(content: &ContentView<'_>) -> Vec<Compilation> {
    let mut out = Vec::new();
    for pom in content.matching_paths() {
        let Some(bytes) = content.read(pom) else {
            continue;
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        out.extend(declared_in(text, kndo_core::paths::dirname(pom.0.as_str())));
    }
    out
}

/// The compilations one pom declares, with `${basedir}` resolved against `module_dir` — the
/// directory of the pom itself, which is what Maven means by it.
///
/// Walks every `<plugin>` element in the document rather than a fixed path: in spring-petclinic
/// the element sits at `project > profiles > profile > build > plugins > plugin`, four levels
/// deeper than the `<build>` a fixed lookup would try, and a project may well declare the same
/// compilation in more than one profile.
fn declared_in(pom: &str, module_dir: &str) -> Vec<Compilation> {
    // Some poms carry a DOCTYPE; roxmltree refuses those unless told otherwise, and a
    // DOCTYPE-less fixture cannot catch a real pom silently failing to parse — the same
    // failure mode `kndo:info-plist` guards against. Safe: roxmltree never resolves external
    // entities.
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let Ok(doc) = roxmltree::Document::parse_with_options(pom, options) else {
        return Vec::new(); // unparseable: degrade to silence, never to a guess
    };
    let mut out = Vec::new();
    for plugin in doc
        .descendants()
        .filter(|n| n.has_tag_name("plugin"))
        .filter(|n| child_text(n, "artifactId").is_some_and(|a| a == ARTIFACT_ID))
    {
        let Some(configuration) = plugin.children().find(|n| n.has_tag_name("configuration"))
        else {
            continue;
        };
        let resolve = |element: &str| {
            child_text(&configuration, element).and_then(|v| project_relative(v, module_dir))
        };
        if let (Some(input), Some(output)) = (resolve("inputPath"), resolve("outputPath")) {
            let compilation = Compilation { input, output };
            if !out.contains(&compilation) {
                out.push(compilation);
            }
        }
    }
    out
}

fn child_text<'a>(node: &roxmltree::Node<'a, 'a>, tag: &str) -> Option<&'a str> {
    node.children()
        .find(|n| n.has_tag_name(tag))
        .and_then(|n| n.text())
        .map(str::trim)
}

/// A configured directory as a project-relative path, or `None` when it names something this
/// plugin cannot resolve.
///
/// `${basedir}` is the pom's own directory and is the one interpolation Maven guarantees.
/// Anything else — `${project.build.directory}`, a user property — depends on a build kndo
/// never runs, so it yields nothing rather than a guess. An absolute path is likewise outside
/// the project and names no file kndo has.
fn project_relative(configured: &str, module_dir: &str) -> Option<String> {
    let rest = configured.trim().strip_prefix("${basedir}")?;
    if rest.contains("${") {
        return None;
    }
    let joined = kndo_core::paths::join(module_dir, rest.trim_start_matches('/'));
    (!joined.is_empty()).then_some(joined)
}

/// Is `path` inside directory `dir`? Segment-wise, so `static/cssx/a.css` is not under
/// `static/css`.
fn is_under(path: &str, dir: &str) -> bool {
    let dir = dir.trim_end_matches('/');
    match path.strip_prefix(dir) {
        Some(rest) => rest.starts_with('/'),
        None => dir.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// spring-petclinic's own declaration, at its real depth.
    const PETCLINIC: &str = r#"<project>
  <profiles>
    <profile>
      <id>css</id>
      <build>
        <plugins>
          <plugin>
            <groupId>com.gitlab.haynes</groupId>
            <artifactId>libsass-maven-plugin</artifactId>
            <configuration>
              <inputPath>${basedir}/src/main/scss/</inputPath>
              <outputPath>${basedir}/src/main/resources/static/resources/css/</outputPath>
              <includePath>${project.build.directory}/webjars/</includePath>
            </configuration>
          </plugin>
        </plugins>
      </build>
    </profile>
  </profiles>
</project>"#;

    #[test]
    fn descriptor_claims_the_reserved_namespace_and_gates_on_sass() {
        let d = LibsassMavenPlugin.descriptor();
        assert_eq!(d.id, "kndo:libsass-maven-plugin");
        assert!(kndo_core::plugin::is_reserved_id(&d.id));
        assert_eq!(
            d.activation,
            vec![ActivationRule::FileExists(SmolStr::new("**/*.scss"))]
        );
        assert_eq!(d.requested_file_access, vec![SmolStr::new("**/pom.xml")]);
        assert!(LibsassMavenPlugin.mutates_graph());
    }

    #[test]
    fn the_petclinic_declaration_is_found_four_levels_inside_a_profile() {
        // The reason this walks every `<plugin>` element instead of `project > build > plugins`:
        // a fixed path would find nothing here, and finding nothing looks exactly like a
        // project that doesn't use the tool.
        assert_eq!(
            declared_in(PETCLINIC, ""),
            vec![Compilation {
                input: "src/main/scss".to_string(),
                output: "src/main/resources/static/resources/css".to_string(),
            }]
        );
    }

    #[test]
    fn a_module_pom_resolves_basedir_against_its_own_directory() {
        assert_eq!(
            declared_in(PETCLINIC, "modules/web"),
            vec![Compilation {
                input: "modules/web/src/main/scss".to_string(),
                output: "modules/web/src/main/resources/static/resources/css".to_string(),
            }]
        );
    }

    #[test]
    fn a_pom_without_this_plugin_declares_nothing() {
        let other = r#"<project><build><plugins>
            <plugin><artifactId>maven-compiler-plugin</artifactId>
              <configuration><inputPath>${basedir}/x/</inputPath>
                             <outputPath>${basedir}/y/</outputPath></configuration></plugin>
        </plugins></build></project>"#;
        assert!(declared_in(other, "").is_empty());
        assert!(declared_in("not xml at all", "").is_empty());
        assert!(declared_in("", "").is_empty());
    }

    #[test]
    fn an_interpolation_maven_alone_cannot_resolve_yields_nothing() {
        // `${basedir}` is the one Maven guarantees without running a build. Everything else
        // depends on a build kndo never runs, so it degrades to silence rather than a guess —
        // note `includePath` above is exactly such a value, and is never read anyway.
        assert_eq!(
            project_relative("${basedir}/a/b/", ""),
            Some("a/b".to_string())
        );
        assert_eq!(
            project_relative("${basedir}/a", "m"),
            Some("m/a".to_string())
        );
        assert_eq!(project_relative("${project.build.directory}/x", ""), None);
        assert_eq!(project_relative("${basedir}/${css.dir}/x", ""), None);
        assert_eq!(project_relative("/absolute/path", ""), None);
        assert_eq!(project_relative("${basedir}", ""), None);
    }

    #[test]
    fn containment_is_segment_wise() {
        assert!(is_under("static/css/a.css", "static/css"));
        assert!(is_under("static/css/a.css", "static/css/"));
        assert!(is_under("static/css/deep/a.css", "static/css"));
        assert!(!is_under("static/cssx/a.css", "static/css"));
        assert!(!is_under("static/css", "static/css"));
        assert!(!is_under("elsewhere/a.css", "static/css"));
    }

    #[test]
    fn a_second_profile_declaring_the_same_compilation_is_not_counted_twice() {
        let twice = PETCLINIC.replace("</profile>\n  </profiles>", "</profile>\n    <profile><id>b</id><build><plugins><plugin><artifactId>libsass-maven-plugin</artifactId><configuration><inputPath>${basedir}/src/main/scss/</inputPath><outputPath>${basedir}/src/main/resources/static/resources/css/</outputPath></configuration></plugin></plugins></build></profile>\n  </profiles>");
        assert_eq!(declared_in(&twice, "").len(), 1);
    }
}
