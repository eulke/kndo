//! `kndo:spring` — what the Spring container constructs and invokes.
//!
//! A Spring bean is a class nothing in the project ever names: the container
//! finds it by component scan, constructs it, and invokes its handler methods
//! when a request, an event or a schedule arrives. Every one of those is an
//! entry outside the code graph's sight, which is why the annotation is the
//! only evidence there is — and why, without this pack, a whole application
//! module reads as dead.
//!
//! Measured on the pinned corpus: 7 findings in Exposed's
//! `samples/springboot3-exposed-r2dbc` module — three `@Configuration` /
//! `@RestController` files reported `unused` outright and four bean classes
//! with them. Nothing else in the corpus carries a Spring annotation.

use kndo_contract::evidence::{RootKind, SymbolKind};
use kndo_contract::extension::{DispatchRule, Effect, Extension, ExtensionSpec, Trigger};
use kndo_contract::vocab::Confidence;
use std::sync::LazyLock;

/// The stereotypes a component scan instantiates. Spelled as the annotation is
/// written, which is how the marker arrives: the engine matches the marker path
/// the language reported, and a JVM annotation is written by its simple name.
const STEREOTYPES: &[&str] = &[
    "Component",
    "Service",
    "Repository",
    "Controller",
    "RestController",
    "Configuration",
    "ControllerAdvice",
    "RestControllerAdvice",
    "SpringBootApplication",
    "ConfigurationProperties",
];

/// Methods the container itself calls on a bean it already holds. These are
/// WITNESSES, not roots: the container does not enter the program here — it
/// holds the bean and dispatches through it — so the method is alive while its
/// owner is, and paints its file no colour the source never claimed.
const HANDLERS: &[&str] = &[
    "Bean",
    "EventListener",
    "TransactionalEventListener",
    "Scheduled",
    "PostConstruct",
    "PreDestroy",
    "ExceptionHandler",
    "InitBinder",
    "ModelAttribute",
    "RequestMapping",
    "GetMapping",
    "PostMapping",
    "PutMapping",
    "DeleteMapping",
    "PatchMapping",
    "MessageMapping",
    "KafkaListener",
    "RabbitListener",
    "JmsListener",
];

static SPEC: LazyLock<ExtensionSpec> = LazyLock::new(|| {
    let mut rules: Vec<DispatchRule> = STEREOTYPES
        .iter()
        .map(|marker| DispatchRule {
            // On a TYPE: `@Component` on anything else is not a bean, and the
            // rule says so rather than trusting each JVM grammar to have put
            // the annotation nowhere it does not belong.
            when: Trigger::marker_on(marker, SymbolKind::Type),
            then: Effect::Root(RootKind::Production),
            // The annotation is the code's own statement.
            confidence: Confidence::Certain,
        })
        .collect();
    rules.extend(HANDLERS.iter().map(|marker| DispatchRule {
        when: Trigger::marker_on(marker, SymbolKind::Method),
        then: Effect::Witness,
        confidence: Confidence::Certain,
    }));
    ExtensionSpec::builder("kndo:spring", 1)
        // A marker is a bare NAME: `@Controller` is Spring's here and Vapor's
        // on a Swift file, and nothing in the name tells them apart. The pack
        // says whose files it speaks for.
        .rules_for(&["kndo:java", "kndo:kotlin"])
        .dispatch(rules)
        .build()
});

/// The Spring pack: stereotypes root, handlers witness.
pub struct SpringRules;

impl Extension for SpringRules {
    fn spec(&self) -> &ExtensionSpec {
        &SPEC
    }
}
