//! The built-in `kndo:coverage-*` ingesters — lcov, Cobertura XML, JaCoCo XML, and Go
//! coverprofile, one plugin type per report format.
//!
//! These live in their own plugin crate, not in `kndo-core`, for the same reason
//! `kndo-plugin-express` does: core knows no language *and no report format* (RFC 0001's
//! ignorance rule) — it owns only the format-neutral model (`kndo_core::coverage`) and the
//! `ingest_coverage` hook. Every ingester here is a plain [`Plugin`] ("the same trait serves
//! built-ins"), statically linked into the product by `kndo::default_plugins()`; formats kndo
//! doesn't ship come in as external WASM components through the same hook.
//!
//! Shared charter, per format, is "the subset that matters": the line → hit-count facts
//! `crap` needs, nothing else. Branch/function/method records are ignored everywhere —
//! kndo maps lines to functions itself via symbol spans. And a shared honesty rule for path
//! keys: a parser normalizes as far as *it* can know (separators, `./`), emits candidate keys
//! when the true source root is ambiguous (JaCoCo), and leaves root- and package-relative
//! rebasing to the host, which alone knows the project — a key that matches no graph path
//! matches nothing: degrade to silence, never to a wrong file. Emitting a wrong candidate is
//! harmless by construction: lookups are by exact graph path, and even a post-rebase
//! collision only accumulates hit counts, which consumers read as `hits > 0`.
//!
//! Every ingester declares `activation: vec![]` (always-on). A `FileExists` gate here would
//! deactivate the plugin at composition time exactly when `kndo.toml` points `report` at a
//! non-well-known path — composition is structural and never sees config. Always-on costs a
//! handful of `stat` calls on report-less projects; `mutates_graph() == false` keeps both
//! graph fast paths alive regardless.

use kndo_core::adapter::ProjectPath;
use kndo_core::coverage::CoverageSink;
use kndo_core::plugin::{Plugin, PluginDescriptor};
use smol_str::SmolStr;

/// The built-in lcov ingester (lcov is the coverage lingua franca:
/// jest/vitest/nyc, llvm-cov, gcov, Go via converters).
pub struct LcovPlugin;

impl Plugin for LcovPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            // Built-ins live in the reserved `kndo:` namespace.
            id: SmolStr::new("kndo:coverage-lcov"),
            version: SmolStr::new("1"),
            // Prose because `activation` below cannot express this gate: the plugin is
            // always-on and what it actually looks for is a set of report paths.
            detection: vec![SmolStr::new("an lcov.info file at a well-known path")],
            // Well-known locations — overridden, NOT extended, by a `[plugins.<id>] report`
            // entry in kndo.toml.
            requested_file_access: vec![
                SmolStr::new("coverage/lcov.info"),
                SmolStr::new("lcov.info"),
            ],
            // Always-on: see the module doc.
            activation: vec![],
            dependencies: vec![],
        }
    }

    /// Coverage ingestion only — no graph-mutation hooks. Without this override, this plugin's
    /// unconditional registration in `default_plugins()` would force every real `kndo` run to
    /// bypass the graph-snapshot cache and the incremental patch (see the trait method's
    /// doc) — both fast paths would be silently dead in the shipped product.
    fn mutates_graph(&self) -> bool {
        false
    }

    /// The lcov subset that matters: `SF:<path>` opens a file section, `DA:<line>,<hits>`
    /// records one instrumented line, `end_of_record` closes it — everything else (function/
    /// branch records, checksums) is ignored. `SF:` paths are kept as reported (`./` and
    /// backslash normalization only) — the plugin doesn't know the project root; the host
    /// rebases absolute keys onto it afterwards (`CoverageMap::rebase`), for every ingesting
    /// plugin uniformly.
    fn ingest_coverage(&self, _path: &ProjectPath, content: &[u8], out: &mut CoverageSink) {
        let Ok(text) = std::str::from_utf8(content) else {
            return;
        };
        let mut current: Option<ProjectPath> = None;
        for line in text.lines() {
            let line = line.trim_end();
            if let Some(sf) = line.strip_prefix("SF:") {
                let normalized = sf.trim().trim_start_matches("./").replace('\\', "/");
                current = Some(ProjectPath(SmolStr::new(normalized)));
            } else if let Some(da) = line.strip_prefix("DA:") {
                if let Some(file) = &current {
                    let mut parts = da.splitn(3, ',');
                    let line_no = parts.next().and_then(|s| s.trim().parse::<u32>().ok());
                    let hits = parts.next().and_then(|s| s.trim().parse::<u64>().ok());
                    if let (Some(line_no), Some(hits)) = (line_no, hits) {
                        out.add_line(file.clone(), line_no, hits);
                    }
                }
            } else if line == "end_of_record" {
                current = None;
            }
        }
    }
}

/// The built-in Cobertura XML ingester (coverage.py's `coverage xml`, many .NET tools,
/// istanbul/nyc's cobertura reporter).
pub struct CoberturaPlugin;

impl Plugin for CoberturaPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: SmolStr::new("kndo:coverage-cobertura"),
            version: SmolStr::new("1"),
            // Prose because `activation` below cannot express this gate: the plugin is
            // always-on and what it actually looks for is a set of report paths.
            detection: vec![SmolStr::new("a Cobertura XML report at a well-known path")],
            // Well-known locations — overridden, NOT extended, by a `[plugins.<id>] report`
            // entry in kndo.toml.
            requested_file_access: vec![
                SmolStr::new("coverage.xml"),
                SmolStr::new("cobertura.xml"),
                SmolStr::new("coverage/cobertura-coverage.xml"),
            ],
            // Always-on: see the module doc.
            activation: vec![],
            dependencies: vec![],
        }
    }

    /// See [`LcovPlugin::mutates_graph`] — same reasoning for every ingester here.
    fn mutates_graph(&self) -> bool {
        false
    }

    /// The Cobertura subset that matters: every `<class filename="...">`'s descendant
    /// `<line number hits>` elements. `<sources><source>` entries are prefixes writers use
    /// for the filenames; which one applies isn't recorded per class, so each filename is
    /// emitted verbatim *and* joined under every source (candidate keys — the module doc's
    /// honesty rule; absolute sources land project-relative via the host's rebase).
    /// `branch`/`condition-coverage`/`<methods>` are ignored.
    fn ingest_coverage(&self, _path: &ProjectPath, content: &[u8], out: &mut CoverageSink) {
        let Ok(text) = std::str::from_utf8(content) else {
            return;
        };
        let Ok(doc) = roxmltree::Document::parse_with_options(text, xml_options()) else {
            return;
        };
        let sources: Vec<String> = doc
            .descendants()
            .filter(|n| n.has_tag_name("source"))
            .filter_map(|n| n.text())
            .map(|s| s.trim().trim_end_matches(['/', '\\']).replace('\\', "/"))
            .filter(|s| !s.is_empty())
            .collect();
        for class in doc.descendants().filter(|n| n.has_tag_name("class")) {
            let Some(filename) = class.attribute("filename") else {
                continue;
            };
            let filename = filename.trim_start_matches("./").replace('\\', "/");
            let mut keys: Vec<ProjectPath> = Vec::with_capacity(1 + sources.len());
            keys.push(ProjectPath(SmolStr::new(&filename)));
            for source in &sources {
                keys.push(ProjectPath(SmolStr::new(format!("{source}/{filename}"))));
            }
            for line in class.descendants().filter(|n| n.has_tag_name("line")) {
                let number = line.attribute("number").and_then(|v| v.parse::<u32>().ok());
                let hits = line.attribute("hits").and_then(|v| v.parse::<u64>().ok());
                if let (Some(number), Some(hits)) = (number, hits) {
                    for key in &keys {
                        out.add_line(key.clone(), number, hits);
                    }
                }
            }
        }
    }
}

/// Every XML kndo reads is written by a real tool, and real tools emit a DOCTYPE: JaCoCo
/// declares `report PUBLIC "-//JACOCO//DTD Report 1.1//EN"` on every report it writes,
/// Cobertura a SYSTEM identifier, an Apple `Info.plist` the PropertyList DTD. `roxmltree`
/// refuses those outright by default (`XML with DTD detected`), so parsing without this
/// option means every real report silently ingests NOTHING while a DOCTYPE-less fixture
/// passes — which is exactly how it went unnoticed. Safe to allow: roxmltree never resolves
/// external entities and caps internal expansion, so no document can reach the network or
/// the filesystem through this.
fn xml_options() -> roxmltree::ParsingOptions {
    roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    }
}

/// The built-in JaCoCo XML ingester (Gradle's `jacocoTestReport`, Maven's `jacoco:report`).
pub struct JacocoPlugin;

impl Plugin for JacocoPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: SmolStr::new("kndo:coverage-jacoco"),
            version: SmolStr::new("1"),
            // Prose because `activation` below cannot express this gate: the plugin is
            // always-on and what it actually looks for is a set of report paths.
            detection: vec![SmolStr::new("a JaCoCo XML report at a well-known path")],
            // Well-known locations — overridden, NOT extended, by a `[plugins.<id>] report`
            // entry in kndo.toml.
            requested_file_access: vec![
                SmolStr::new("build/reports/jacoco/test/jacocoTestReport.xml"),
                SmolStr::new("target/site/jacoco/jacoco.xml"),
                SmolStr::new("jacoco.xml"),
            ],
            // Always-on: see the module doc.
            activation: vec![],
            dependencies: vec![],
        }
    }

    /// See [`LcovPlugin::mutates_graph`].
    fn mutates_graph(&self) -> bool {
        false
    }

    /// The JaCoCo subset that matters: `<package name>` / `<sourcefile name>` /
    /// `<line nr ci>`, where `ci` (covered instructions) > 0 ⇔ the line executed — the only
    /// property consumers read. JaCoCo reports Java-package paths, not source paths; the
    /// real file lives under a source root only the build layout knows. Candidate keys
    /// (module doc's honesty rule): `{module}{root}{package}/{file}` for the standard JVM
    /// roots of the two JVM adapters kndo ships (`src/main/java/`, `src/main/kotlin/`) plus
    /// the bare join, with `{module}` derived from the report's own location — its path
    /// prefix before the `build/` or `target/` segment (empty for a root-level report).
    fn ingest_coverage(&self, path: &ProjectPath, content: &[u8], out: &mut CoverageSink) {
        let Ok(text) = std::str::from_utf8(content) else {
            return;
        };
        let Ok(doc) = roxmltree::Document::parse_with_options(text, xml_options()) else {
            return;
        };
        let module = module_prefix(path.0.as_str());
        const SOURCE_ROOTS: [&str; 3] = ["src/main/java/", "src/main/kotlin/", ""];
        for package in doc.descendants().filter(|n| n.has_tag_name("package")) {
            let Some(pkg) = package.attribute("name") else {
                continue;
            };
            for sourcefile in package.children().filter(|n| n.has_tag_name("sourcefile")) {
                let Some(file) = sourcefile.attribute("name") else {
                    continue;
                };
                let rel = if pkg.is_empty() {
                    file.to_string()
                } else {
                    format!("{pkg}/{file}")
                };
                let keys: Vec<ProjectPath> = SOURCE_ROOTS
                    .iter()
                    .map(|root| ProjectPath(SmolStr::new(format!("{module}{root}{rel}"))))
                    .collect();
                for line in sourcefile.children().filter(|n| n.has_tag_name("line")) {
                    let nr = line.attribute("nr").and_then(|v| v.parse::<u32>().ok());
                    let ci = line.attribute("ci").and_then(|v| v.parse::<u64>().ok());
                    if let (Some(nr), Some(ci)) = (nr, ci) {
                        for key in &keys {
                            out.add_line(key.clone(), nr, ci);
                        }
                    }
                }
            }
        }
    }
}

/// The report path's prefix before its `build/` or `target/` segment, `/`-terminated —
/// the module directory in a Gradle/Maven multi-module layout (`app/build/reports/...` ⇒
/// `app/`), empty for a root-level report.
fn module_prefix(report_path: &str) -> String {
    for marker in ["build/", "target/"] {
        if let Some(idx) = report_path.find(marker) {
            return report_path[..idx].to_string();
        }
    }
    String::new()
}

/// The built-in Go coverprofile ingester (`go test -coverprofile=coverage.out`).
pub struct GoCoverPlugin;

impl Plugin for GoCoverPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: SmolStr::new("kndo:coverage-go"),
            version: SmolStr::new("1"),
            // Prose because `activation` below cannot express this gate: the plugin is
            // always-on and what it actually looks for is a set of report paths.
            detection: vec![SmolStr::new("a Go coverprofile at a well-known path")],
            // Well-known locations — overridden, NOT extended, by a `[plugins.<id>] report`
            // entry in kndo.toml.
            requested_file_access: vec![SmolStr::new("coverage.out"), SmolStr::new("cover.out")],
            // Always-on: see the module doc.
            activation: vec![],
            dependencies: vec![],
        }
    }

    /// See [`LcovPlugin::mutates_graph`].
    fn mutates_graph(&self) -> bool {
        false
    }

    /// The coverprofile subset that matters: skip the `mode:` header; each block line is
    /// `path.go:SL.SC,EL.EC STMTS COUNT` — every line in `SL..=EL` gets `COUNT` (line-level
    /// approximation of statement blocks, columns ignored — same posture as lcov's
    /// "line-level ⇒ statement-level approximation"). Paths are module-qualified
    /// (`github.com/x/y/pkg/file.go`) and recorded verbatim: only the host knows which
    /// directory a module maps to, and rebases them from the graph's package table.
    /// Overlapping blocks on one line accumulate (the sink's existing rule).
    fn ingest_coverage(&self, _path: &ProjectPath, content: &[u8], out: &mut CoverageSink) {
        let Ok(text) = std::str::from_utf8(content) else {
            return;
        };
        for line in text.lines() {
            let line = line.trim_end();
            if line.is_empty() || line.starts_with("mode:") {
                continue;
            }
            let Some((file, rest)) = line.rsplit_once(':') else {
                continue;
            };
            let mut fields = rest.split_whitespace();
            let (Some(span), Some(_stmts), Some(count)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let Some((start, end)) = span.split_once(',') else {
                continue;
            };
            let parse_line = |pos: &str| {
                pos.split_once('.')
                    .and_then(|(l, _col)| l.parse::<u32>().ok())
            };
            let (Some(start_line), Some(end_line), Ok(hits)) =
                (parse_line(start), parse_line(end), count.parse::<u64>())
            else {
                continue;
            };
            let key = ProjectPath(SmolStr::new(file.replace('\\', "/")));
            for l in start_line..=end_line.max(start_line) {
                out.add_line(key.clone(), l, hits);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kndo_core::adapter::Span;
    use kndo_core::coverage::CoverageMap;

    fn span(start: u32, end: u32) -> Span {
        Span {
            start: (start, 1),
            end: (end, 1),
        }
    }

    fn ingest(plugin: &dyn Plugin, report_path: &str, content: &str) -> CoverageMap {
        let mut sink = CoverageSink::default();
        plugin.ingest_coverage(
            &ProjectPath(SmolStr::new(report_path)),
            content.as_bytes(),
            &mut sink,
        );
        sink.into_map()
    }

    fn cov(map: &CoverageMap, path: &str, lines: (u32, u32)) -> Option<f64> {
        map.function_coverage(&ProjectPath(SmolStr::new(path)), span(lines.0, lines.1))
    }

    // --- lcov ---

    #[test]
    fn lcov_parses_sections_and_normalizes_sf_paths() {
        let map = ingest(
            &LcovPlugin,
            "lcov.info",
            "TN:\nSF:./src\\a.ts\nDA:1,1\nDA:2,0\nend_of_record\nSF:src/b.ts\nDA:5,3\nend_of_record\n",
        );
        assert!((cov(&map, "src/a.ts", (1, 4)).unwrap() - 0.5).abs() < 1e-9);
        assert!((cov(&map, "src/b.ts", (1, 9)).unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn lcov_ignores_records_outside_a_section_and_non_utf8() {
        let map = ingest(&LcovPlugin, "lcov.info", "DA:1,1\nFN:2,foo\n");
        assert!(map.is_empty());
        let mut sink = CoverageSink::default();
        LcovPlugin.ingest_coverage(
            &ProjectPath(SmolStr::new("lcov.info")),
            &[0xff, 0xfe],
            &mut sink,
        );
        assert!(sink.into_map().is_empty());
    }

    // --- Cobertura ---

    #[test]
    fn cobertura_reads_class_lines_and_joins_sources_as_candidates() {
        let map = ingest(
            &CoberturaPlugin,
            "coverage.xml",
            r#"<?xml version="1.0"?>
<coverage><sources><source>/abs/proj</source></sources>
  <packages><package name="p"><classes>
    <class name="a" filename="src/a.py">
      <methods/>
      <lines><line number="3" hits="2"/><line number="4" hits="0" branch="true"/></lines>
    </class>
  </classes></package></packages>
</coverage>"#,
        );
        // Verbatim key and source-joined candidate both carry the same facts.
        assert!((cov(&map, "src/a.py", (1, 9)).unwrap() - 0.5).abs() < 1e-9);
        assert!((cov(&map, "/abs/proj/src/a.py", (1, 9)).unwrap() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn the_doctype_every_real_report_carries_is_read_not_refused() {
        // roxmltree's default options REFUSE a document declaring a DTD. Every JaCoCo report
        // declares one and Cobertura's writer emits a SYSTEM identifier, so both must parse
        // with `xml_options()` or silently ingest nothing from a real report — a fixture
        // without a DOCTYPE cannot catch that.
        let jacoco = ingest(
            &JacocoPlugin,
            "build/reports/jacoco/test/jacocoTestReport.xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<!DOCTYPE report PUBLIC "-//JACOCO//DTD Report 1.1//EN" "report.dtd">
<report name="app"><package name="com/acme">
  <sourcefile name="Thing.java"><line nr="3" mi="0" ci="2"/><line nr="4" mi="1" ci="0"/></sourcefile>
</package></report>"#,
        );
        assert!(
            !jacoco.is_empty(),
            "a JaCoCo report's DOCTYPE must not make the whole report invisible"
        );

        let cobertura = ingest(
            &CoberturaPlugin,
            "coverage.xml",
            r#"<?xml version="1.0" ?>
<!DOCTYPE coverage SYSTEM "http://cobertura.sourceforge.net/xml/coverage-04.dtd">
<coverage><packages><package name="p"><classes>
    <class name="a" filename="src/a.py"><lines><line number="3" hits="2"/></lines></class>
</classes></package></packages></coverage>"#,
        );
        assert!(
            !cobertura.is_empty(),
            "same for Cobertura's SYSTEM identifier"
        );
    }

    #[test]
    fn cobertura_malformed_xml_degrades_to_an_empty_map() {
        assert!(ingest(&CoberturaPlugin, "coverage.xml", "<coverage><unclosed").is_empty());
    }

    // --- JaCoCo ---

    #[test]
    fn jacoco_emits_source_root_candidates_with_the_module_prefix() {
        let map = ingest(
            &JacocoPlugin,
            "app/build/reports/jacoco/test/jacocoTestReport.xml",
            r#"<report name="x"><package name="com/example">
                 <sourcefile name="Foo.kt">
                   <line nr="7" mi="0" ci="4"/><line nr="8" mi="2" ci="0"/>
                 </sourcefile>
               </package></report>"#,
        );
        let hit = cov(&map, "app/src/main/kotlin/com/example/Foo.kt", (1, 20)).unwrap();
        assert!((hit - 0.5).abs() < 1e-9);
        // The other candidates exist but are dead keys unless the graph has such a path.
        assert!(cov(&map, "app/com/example/Foo.kt", (1, 20)).is_some());
        assert!(cov(&map, "src/main/kotlin/com/example/Foo.kt", (1, 20)).is_none());
    }

    #[test]
    fn jacoco_root_level_report_has_no_module_prefix() {
        let map = ingest(
            &JacocoPlugin,
            "jacoco.xml",
            r#"<report><package name="p"><sourcefile name="A.java">
                 <line nr="1" ci="1"/></sourcefile></package></report>"#,
        );
        assert!(cov(&map, "src/main/java/p/A.java", (1, 5)).is_some());
    }

    // --- Go coverprofile ---

    #[test]
    fn go_coverprofile_expands_block_ranges_verbatim_module_paths() {
        let map = ingest(
            &GoCoverPlugin,
            "coverage.out",
            "mode: set\ngithub.com/x/y/pkg/a.go:3.2,5.10 2 1\ngithub.com/x/y/pkg/a.go:8.1,8.20 1 0\n",
        );
        let c = cov(&map, "github.com/x/y/pkg/a.go", (1, 10)).unwrap();
        // Lines 3,4,5 hit; line 8 not: 3 of 4 instrumented.
        assert!((c - 0.75).abs() < 1e-9);
    }

    #[test]
    fn go_coverprofile_skips_malformed_lines() {
        let map = ingest(
            &GoCoverPlugin,
            "coverage.out",
            "mode: atomic\nnot a block line\nfile.go:bad\nfile.go:1.1,2.1 1 5\n",
        );
        assert!((cov(&map, "file.go", (1, 5)).unwrap() - 1.0).abs() < 1e-9);
    }

    // --- shared descriptor posture ---

    #[test]
    fn every_ingester_is_always_on_and_mutation_free() {
        let plugins: [&dyn Plugin; 4] =
            [&LcovPlugin, &CoberturaPlugin, &JacocoPlugin, &GoCoverPlugin];
        for p in plugins {
            let d = p.descriptor();
            assert!(d.activation.is_empty(), "{} must be always-on", d.id);
            assert!(d.id.starts_with("kndo:coverage-"));
            assert!(!d.requested_file_access.is_empty());
            assert!(
                !p.mutates_graph(),
                "{} must keep the fast paths alive",
                d.id
            );
        }
    }
}
