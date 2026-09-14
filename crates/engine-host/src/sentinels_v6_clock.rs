//! Scheduler debt, independent of game economy and fixed simulation timestep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Domain {
    Stopped,
    Legacy,
    V6(u64),
}
#[derive(Default)]
pub(crate) struct Clock {
    domain: Option<Domain>,
    debt: f64,
}
pub(crate) const MAX_STEPS_PER_ITERATION: usize = 2;
impl Clock {
    pub fn observe(&mut self, elapsed: f64, domain: Domain) {
        if self.domain != Some(domain) {
            self.domain = Some(domain);
            self.debt = 0.;
            return;
        }
        if domain == Domain::Stopped {
            self.debt = 0.;
            return;
        }
        if elapsed.is_finite() && elapsed >= 0. {
            self.debt += elapsed;
        }
        if domain == Domain::Legacy {
            self.debt = self.debt.min(0.25);
        }
    }
    pub fn due(&self, dt: f64) -> bool {
        self.debt >= dt
    }
    pub fn consume(&mut self, dt: f64) {
        self.debt = (self.debt - dt).max(0.);
    }
    pub fn debt(&self) -> f64 {
        self.debt
    }
}
pub(crate) fn domain(st: &crate::rpc::HostState) -> Domain {
    if st.play == crate::rpc::PlayState::Paused {
        return Domain::Stopped;
    }
    if let Some(session) = &st.sentinels_v6 {
        let Some(game) = &session.game else {
            return Domain::Stopped;
        };
        if let Some(replay) = &session.playback {
            if replay.paused && replay.seek.is_none() {
                return Domain::Stopped;
            }
        } else if game.state.winner.is_some() {
            return Domain::Stopped;
        }
        Domain::V6(session.clock_epoch)
    } else {
        Domain::Legacy
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const DT: f64 = 1. / 60.;
    #[test]
    fn v6_retains_a_two_second_stall_and_consumes_only_fixed_bounded_steps() {
        let mut clock = Clock::default();
        clock.observe(0., Domain::V6(7));
        clock.observe(2., Domain::V6(7));
        for _ in 0..MAX_STEPS_PER_ITERATION {
            assert!(clock.due(DT));
            clock.consume(DT);
        }
        assert!((clock.debt() - (2. - 2. * DT)).abs() < 1e-10);
        let mut steps = 2;
        while clock.due(DT) {
            clock.consume(DT);
            steps += 1;
        }
        assert!((119..=120).contains(&steps));
        assert!((steps as f64 * DT + clock.debt() - 2.).abs() < 1e-10);
    }
    #[test]
    fn pause_resume_and_replaced_v6_sessions_do_not_inherit_time_debt() {
        let mut clock = Clock::default();
        clock.observe(0., Domain::V6(1));
        clock.observe(1., Domain::V6(1));
        clock.observe(3., Domain::Stopped);
        clock.observe(20., Domain::Stopped);
        assert_eq!(clock.debt(), 0.);
        clock.observe(5., Domain::V6(1));
        assert_eq!(clock.debt(), 0.);
        clock.observe(1., Domain::V6(1));
        clock.observe(0.1, Domain::V6(2));
        assert_eq!(clock.debt(), 0.);
        clock.observe(DT, Domain::V6(2));
        assert!(clock.due(DT));
    }
    #[test]
    fn legacy_editor_keeps_its_existing_quarter_second_cap() {
        let mut clock = Clock::default();
        clock.observe(0., Domain::Legacy);
        clock.observe(2., Domain::Legacy);
        assert_eq!(clock.debt(), 0.25);
        clock.consume(DT);
        clock.observe(0.001, Domain::Legacy);
        assert!((clock.debt() - (0.25 - DT + 0.001)).abs() < 1e-12);
    }
}
