use crate::{Game, Save};
pub fn apply_replay_orders(g: &mut Game, save: &Save, index: &mut usize, admin: &mut usize) {
    loop {
        if let Some(event) = save.administrativeEvents.get(*admin) {
            if event.tick == g.state.tick && event.orderIndex <= *index {
                let _ = g.forfeit(event.owner, event.reason.clone());
                *admin += 1;
                continue;
            }
        }
        let Some(logged) = save.orders.get(*index).filter(|l| l.tick == g.state.tick) else {
            break;
        };
        if !g.player(logged.order.owner).map(|p| p.ai).unwrap_or(false) {
            g.order(logged.order.clone());
        }
        *index += 1;
    }
}
pub struct ReplayController {
    pub save: Save,
    pub index: usize,
    pub admin: usize,
    pub paused: bool,
    pub speed: f64,
    pub seek: Option<u64>,
    pub phase: f64,
}
impl ReplayController {
    pub fn new(save: Save) -> Result<Self,String> {
        save.validate()?;
        Ok(Self {
            save,
            index: 0,
            admin: 0,
            paused: false,
            speed: 1.,
            seek: None,
            phase: 0.,
        })
    }
    pub fn advance(&mut self, g: &mut Game) {
        let steps = if self.seek.is_some() {
            self.phase = 0.;
            32
        } else if self.paused {
            self.phase = 0.;
            0
        } else {
            self.phase += self.speed;
            let steps = self.phase.floor() as usize;
            self.phase -= steps as f64;
            steps
        };
        for _ in 0..steps {
            apply_replay_orders(g, &self.save, &mut self.index, &mut self.admin);
            let goal = self.seek.unwrap_or(self.save.snapshot.tick);
            if g.state.tick >= goal || g.state.winner.is_some() {
                self.paused = true;
                self.seek = None;
                break;
            }
            g.step();
        }
        apply_replay_orders(g, &self.save, &mut self.index, &mut self.admin);
        if g.state.tick >= self.seek.unwrap_or(self.save.snapshot.tick) || g.state.winner.is_some()
        {
            self.paused = true;
            self.seek = None;
            self.phase = 0.;
        }
    }
    pub fn rewind(&mut self) -> Game {
        self.index = 0;
        self.admin = 0;
        self.phase = 0.;
        Game::new_ruleset(
            self.save.snapshot.seed,
            self.save.initialAi,
            &self.save.snapshot.theme,
            &self.save.snapshot.ruleset,
        )
    }
}
