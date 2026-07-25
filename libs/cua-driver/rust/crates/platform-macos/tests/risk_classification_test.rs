//! Every registered tool must carry a reviewed risk classification.
//!
//! `authorize_tool_call` fails closed on `RiskClass::Unclassified`, so a tool
//! that registers without a classification compiles, appears in `tools/list`,
//! and then denies every call at runtime. This test moves that failure to
//! build time, where the person adding the tool sees it.

#![cfg(target_os = "macos")]

use cua_driver_core::authorization::{advertised_risk_for, RiskClass};

#[test]
fn every_registered_tool_has_a_reviewed_risk_class() {
    let registry = platform_macos::register_tools_with_compat(false);
    let unclassified: Vec<&str> = registry
        .tool_names()
        .filter(|name| advertised_risk_for(name).class == RiskClass::Unclassified)
        .collect();

    assert!(
        unclassified.is_empty(),
        "these registered tools have no risk classification and will be denied at \
         invocation: {unclassified:?}. Add each to `advertised_risk_for` in \
         cua-driver-core/src/authorization.rs."
    );
}

#[test]
fn compat_mode_registry_is_also_fully_classified() {
    let registry = platform_macos::register_tools_with_compat(true);
    let unclassified: Vec<&str> = registry
        .tool_names()
        .filter(|name| advertised_risk_for(name).class == RiskClass::Unclassified)
        .collect();

    assert!(
        unclassified.is_empty(),
        "compat-mode tools missing a risk classification: {unclassified:?}"
    );
}
