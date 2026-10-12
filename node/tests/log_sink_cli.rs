use clap::error::ErrorKind;
use clap::Parser;
use node::rust::configuration::commandline::options::OptionsSubCommand;
use node::rust::configuration::Options;

#[test]
fn deployment_sink_precedes_run() {
    let options = Options::try_parse_from(["node", "--log-sink=stdout", "run"]).unwrap();
    assert_eq!(options.log_sink.as_deref(), Some("stdout"));
    assert!(matches!(
        options.subcommand,
        Some(OptionsSubCommand::Run(_))
    ));
}

#[test]
fn file_and_development_sinks_remain_available() {
    for sink in ["file", "both"] {
        let options = Options::try_parse_from(["node", "--log-sink", sink, "run"]).unwrap();
        assert_eq!(options.log_sink.as_deref(), Some(sink));
        assert!(matches!(
            options.subcommand,
            Some(OptionsSubCommand::Run(_))
        ));
    }
}

#[test]
fn misplaced_deployment_sink_is_rejected() {
    let error = Options::try_parse_from(["node", "run", "--log-sink=stdout"])
        .err()
        .unwrap();
    assert_eq!(error.kind(), ErrorKind::UnknownArgument);
}
