//! Maven's own words, read from the shape a real reactor has: guava's, whose
//! two parallel reactors each declare a `guava` and a `guava-tests` and whose
//! source roots live in an inherited parent this reader never sees.

use kndo_contract::adapter::SourceFile;
use kndo_contract::manifest::{ManifestSink, UnitKind};
use kndo_contract::vocab::ProjectPath;

fn read(path: &str, content: &str) -> kndo_contract::manifest::ManifestEvidence {
    let path = ProjectPath::new(path);
    let mut sink = ManifestSink::new();
    kndo_toolkit::jvm_manifest::structure(
        &SourceFile {
            path: &path,
            content: content.as_bytes(),
        },
        &mut sink,
    );
    sink.finish()
}

/// guava's root pom, trimmed to the elements this reader looks at.
const AGGREGATOR: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.google.guava</groupId>
  <artifactId>guava-parent</artifactId>
  <packaging>pom</packaging>
  <modules>
    <module>guava</module>
    <module>guava-testlib</module>
    <module>guava-tests</module>
  </modules>
  <build>
    <sourceDirectory>src</sourceDirectory>
    <testSourceDirectory>test</testSourceDirectory>
  </build>
</project>
"#;

/// guava-tests', trimmed: a parent it inherits from, and a dependency on its
/// sibling by the artifact id both reactors use.
const MODULE: &str = r#"<project>
  <parent>
    <groupId>com.google.guava</groupId>
    <artifactId>guava-parent</artifactId>
    <version>HEAD-jre-SNAPSHOT</version>
  </parent>
  <artifactId>guava-tests</artifactId>
  <!-- a comment naming <artifactId>decoy</artifactId> -->
  <dependencyManagement>
    <dependencies>
      <dependency><artifactId>managed-only</artifactId></dependency>
    </dependencies>
  </dependencyManagement>
  <dependencies>
    <dependency>
      <groupId>com.google.guava</groupId>
      <artifactId>guava</artifactId>
    </dependency>
    <dependency><artifactId>junit</artifactId></dependency>
  </dependencies>
  <build>
    <plugins>
      <plugin><artifactId>build-helper-maven-plugin</artifactId></plugin>
    </plugins>
  </build>
</project>
"#;

#[test]
fn an_aggregator_lists_its_members_and_compiles_nothing() {
    let evidence = read("pom.xml", AGGREGATOR);
    assert!(
        evidence.units.is_empty(),
        "`<packaging>pom</packaging>` builds no artifact"
    );
    assert_eq!(
        evidence.members,
        vec![
            ProjectPath::new("guava/pom.xml"),
            ProjectPath::new("guava-testlib/pom.xml"),
            ProjectPath::new("guava-tests/pom.xml"),
        ]
    );
}

#[test]
fn a_module_names_itself_its_dependencies_and_nothing_it_merely_contains() {
    let evidence = read("guava-tests/pom.xml", MODULE);
    assert_eq!(evidence.units.len(), 1);
    let unit = &evidence.units[0];
    assert_eq!(unit.name, "guava-tests", "not its parent's artifact id");
    assert_eq!(unit.kind, UnitKind::Library);
    assert!(
        unit.roots.is_empty(),
        "the engine defaults a rootless unit to its manifest's directory — \
         guava's real source root lives in an inherited parent"
    );
    assert_eq!(
        unit.depends_on,
        vec!["guava", "junit"],
        "the dependency section only: not dependencyManagement's versions, \
         not a build plugin, not a comment"
    );
}

#[test]
fn members_are_transcribed_under_the_manifests_own_directory() {
    let evidence = read("android/pom.xml", AGGREGATOR);
    assert_eq!(
        evidence.members[0],
        ProjectPath::new("android/guava/pom.xml"),
        "a nested reactor's member is nested too, which is what keeps the \
         two `guava` units apart"
    );
}
