// Console program: agent hooks must be able to wait for it and pipe JSON to it.

fn main() {
    let event = std::env::args().nth(1).unwrap_or_default();
    agent_busylight::hook::run(&event);
}
