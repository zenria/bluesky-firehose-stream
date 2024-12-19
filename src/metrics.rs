use prometheus::{IntCounter, IntCounterVec, Opts};

pub fn create_counter(name: &str, help: &str) -> IntCounter {
    let counter = IntCounter::new(name, help).unwrap();
    prometheus::register(Box::new(counter.clone())).unwrap();
    counter
}

pub fn create_counter_with_labels(name: &str, help: &str, labels: &[&str]) -> IntCounterVec {
    let counter = IntCounterVec::new(Opts::new(name, help), labels).unwrap();
    prometheus::register(Box::new(counter.clone())).unwrap();
    counter
}
