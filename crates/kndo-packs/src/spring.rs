//! `kndo:spring` — what the Spring container constructs and invokes.
//!
//! A Spring bean is a class nothing in the project ever names: the container
//! finds it by component scan, constructs it, and invokes its handler methods
//! when a request, an event or a schedule arrives. Every one of those is an
//! entry outside the code graph's sight, which is why the annotation is the
//! only evidence there is — and why, without this pack, a whole application
//! module reads as dead.
//!
//! Two gates, both the plan's. ACTIVATION: the project depends on
//! `org.springframework*`, so a tree that does not use Spring never consults
//! these rules. QUALIFICATION: each rule names the annotation's full path, and
//! the engine qualifies the marker a file carries through that file's own
//! import bindings before comparing — `@Controller` matches here when it came
//! from `org.springframework.stereotype`, and never because a Swift file spells
//! the same six letters.

use kndo_contract::evidence::{RootKind, SymbolKind};
use kndo_contract::extension::{
    Activation, ActivationRule, DispatchRule, Effect, Extension, ExtensionSpec, MutatesGraph,
    Trigger,
};
use kndo_contract::vocab::Confidence;
use std::sync::LazyLock;

/// The stereotypes a component scan instantiates, by the path a file imports
/// them from. `@ConfigurationProperties` and the two advice annotations are
/// meta-annotated `@Component` in Spring's own source: the container treats
/// them the same, so the rule does.
const STEREOTYPES: &[&str] = &[
    "org.springframework.stereotype.Component",
    "org.springframework.stereotype.Service",
    "org.springframework.stereotype.Repository",
    "org.springframework.stereotype.Controller",
    "org.springframework.web.bind.annotation.RestController",
    "org.springframework.context.annotation.Configuration",
    "org.springframework.web.bind.annotation.ControllerAdvice",
    "org.springframework.web.bind.annotation.RestControllerAdvice",
    "org.springframework.boot.autoconfigure.SpringBootApplication",
    "org.springframework.boot.context.properties.ConfigurationProperties",
];

/// Methods the container itself calls on a bean it holds: a factory method, a
/// lifecycle callback, a listener, a request mapping. The call site is the
/// container's, so the graph never sees it.
const HANDLERS: &[&str] = &[
    "org.springframework.context.annotation.Bean",
    "org.springframework.context.event.EventListener",
    "org.springframework.transaction.event.TransactionalEventListener",
    "org.springframework.scheduling.annotation.Scheduled",
    "jakarta.annotation.PostConstruct",
    "javax.annotation.PostConstruct",
    "jakarta.annotation.PreDestroy",
    "javax.annotation.PreDestroy",
    "org.springframework.web.bind.annotation.ExceptionHandler",
    "org.springframework.web.bind.annotation.InitBinder",
    "org.springframework.web.bind.annotation.ModelAttribute",
    "org.springframework.web.bind.annotation.RequestMapping",
    "org.springframework.web.bind.annotation.GetMapping",
    "org.springframework.web.bind.annotation.PostMapping",
    "org.springframework.web.bind.annotation.PutMapping",
    "org.springframework.web.bind.annotation.DeleteMapping",
    "org.springframework.web.bind.annotation.PatchMapping",
    "org.springframework.messaging.handler.annotation.MessageMapping",
    "org.springframework.kafka.annotation.KafkaListener",
    "org.springframework.amqp.rabbit.annotation.RabbitListener",
    "jakarta.jms.annotation.JmsListener",
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
            // PROBABLE, not Certain: the annotation is the code's own
            // statement that a container MAY construct this, and whether the
            // container is ever started is outside anything kndo can read.
            confidence: Confidence::Probable,
        })
        .collect();
    rules.extend(HANDLERS.iter().map(|marker| DispatchRule {
        when: Trigger::marker_on(marker, SymbolKind::Method),
        then: Effect::Root(RootKind::Production),
        confidence: Confidence::Probable,
    }));
    ExtensionSpec::builder("kndo:spring", 1)
        .dispatch(rules)
        .conduct(
            Activation::AnyRule(vec![ActivationRule::ManifestDependency(
                "org.springframework*".into(),
            )]),
            // The pack runs no code and touches no graph: its rules are data,
            // and the graph cache key carries them and the active set.
            MutatesGraph::No,
        )
        .build()
});

/// The Spring pack: stereotypes and handler methods are the container's entries.
pub struct SpringRules;

impl Extension for SpringRules {
    fn spec(&self) -> &ExtensionSpec {
        &SPEC
    }
}
