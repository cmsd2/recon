fn rendered() -> Scenario<Cmd> {
    Scenario {
        config: Config {
            seed: 4,
            loss: 0.1,
            duplication: 0.0,
            reorder: 0.0,
            latency_min: Duration::from_nanos(10000000),
            latency_max: Duration::from_nanos(10000000),
            reorder_delay: Duration::from_nanos(50000000),
            synchronous: None,
            sessions: false,
            reconnect_interval: Duration::from_nanos(5000000),
            max_steps: 1000000,
        },
        nodes: vec![NodeId(1), NodeId(2), NodeId(3)],
        steps: vec![
            (Duration::from_nanos(0), Step::Command { node: NodeId(1), cmd: SendTo(NodeId(3), 7) }),
            (Duration::from_nanos(5000000), Step::Partition(vec![vec![NodeId(1), NodeId(2)], vec![NodeId(3)]])),
            (Duration::from_nanos(9000000), Step::Heal),
            (Duration::from_nanos(12000000), Step::Crash(NodeId(2))),
        ],
        horizon: Duration::from_nanos(60000000),
    }
}
