fn main(){let g=sentinels_v6::Game::new(1,false);println!("{}",serde_json::to_string(&g.state).unwrap());}
