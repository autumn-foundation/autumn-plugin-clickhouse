use autumn_web::plugin::Plugin;

use super::*;
use crate::config::ClickHouseConfig;

#[test]
fn plugin_name() {
    let plugin = ClickHousePlugin::new();
    assert_eq!(plugin.name(), PLUGIN_NAME);
    assert_eq!(PLUGIN_NAME, "autumn-plugin-clickhouse");
}

#[test]
fn builder_methods_chain() {
    let plugin = ClickHousePlugin::new()
        .config_section("analytics")
        .configure(|c| {
            c.database = "events".into();
        });
    let text = format!("{plugin:?}");
    assert!(text.contains("analytics"), "unexpected debug: {text}");

    let plugin = ClickHousePlugin::new().config(ClickHouseConfig::default());
    let text = format!("{plugin:?}");
    assert!(text.contains("(explicit)"), "unexpected debug: {text}");

    let plugin = ClickHousePlugin::default();
    assert_eq!(plugin.name(), PLUGIN_NAME);
}

#[test]
fn build_registers_without_a_server() {
    let plugin = ClickHousePlugin::new().config(ClickHouseConfig::default());
    let _app = autumn_web::app().plugin(plugin);
}

#[test]
fn build_with_health_check_off_registers() {
    let config = ClickHouseConfig {
        health_check: false,
        ..ClickHouseConfig::default()
    };
    let plugin = ClickHousePlugin::new().config(config);
    let _app = autumn_web::app().plugin(plugin);
}
