//! Deterministic terrain with mirrored strategic access and core-derived safe pads.
use crate::{Game, Pos};
const WIDTH:i32=128;
const HEIGHT:i32=96;
fn set(terrain:&mut[u8],x:i32,y:i32,value:u8){if (0..WIDTH).contains(&x)&&(0..HEIGHT).contains(&y){terrain[(y*WIDTH+x)as usize]=value;}}
fn mirrored(terrain:&mut[u8],x:i32,y:i32,value:u8){for xx in[x,WIDTH-1-x]{for yy in[y,HEIGHT-1-y]{set(terrain,xx,yy,value);}}}
fn road(terrain:&mut[u8],from:Pos,to:Pos){let mut p=from;loop{for dy in -1..=1{for dx in -1..=1{mirrored(terrain,p.x+dx,p.y+dy,3);}}if p.x!=to.x{p.x+=if to.x>p.x{1}else{-1};}else if p.y!=to.y{p.y+=if to.y>p.y{1}else{-1};}else{break;}}}
impl Game{
 pub fn new(seed:u64,ai:bool)->Self{Self::new_theme(seed,ai,"river")}
 pub fn new_theme(seed:u64,ai:bool,requested:&str)->Self{
  let theme=if matches!(requested,"river"|"mining"|"highland"){requested}else{"river"};
  let mut game=Self::base_new(seed,ai);game.state.theme=theme.into();
  // Rebuild every tile. The older base initializer's pads cannot leak into a theme.
  for y in 0..HEIGHT{for x in 0..WIDTH{
   let mx=x.min(WIDTH-1-x);let my=y.min(HEIGHT-1-y);
   let n=((mx as u64*73856093)^(my as u64*19349663)^seed.wrapping_mul(83492791))%101;
   let terrain=match theme{
    "mining"=>if (58..=63).contains(&mx)&&(18..=22).contains(&my){2}else if n<14{1}else if n<32{4}else{0},
    "highland"=>if (10..=13).contains(&mx)&&(18..=22).contains(&my){2}else if n<7{1}else if (mx/10+my/8)%3!=1{4}else{0},
    _=>if (61..=66).contains(&x){2}else if n<4{1}else if n<12{4}else{0},
   };
   game.state.terrain[(y*WIDTH+x)as usize]=terrain;
  }}
  let cores:Vec<_>=game.state.buildings.iter().filter(|b|b.kind=="core").map(|b|b.rect).collect();
  for rect in &cores{
   // Half-open bounds follow the actual 4x4 core footprints: 0..20 / 108..128.
   for y in rect.y-10..rect.y+rect.height+10{for x in rect.x-8..rect.x+rect.width+8{mirrored(&mut game.state.terrain,x,y,0);}}
  }
  let nodes:Vec<_>=game.state.resources.iter().filter(|r|r.kind=="node").map(|r|r.pos).collect();
  for node in &nodes{
   // Shared bridges stay reachable; terrain between these roads remains freely traversable.
   road(&mut game.state.terrain,Pos::new(6,node.y,0),Pos::new(121,node.y,0));
  }
  for core in &cores{
   let center=core.center();
   if let (Some(low),Some(high))=(nodes.iter().map(|p|p.y).min(),nodes.iter().map(|p|p.y).max()){
    road(&mut game.state.terrain,Pos::new(center.x,low,0),Pos::new(center.x,high,0));
   }
   for kind in["ore","coal"]{
    if let Some(resource)=game.state.resources.iter().filter(|r|r.kind==kind).min_by(|a,b|a.pos.distance(center).total_cmp(&b.pos.distance(center))).map(|r|r.pos){road(&mut game.state.terrain,center,resource);}
   }
  }
  for resource in game.state.resources.iter().filter(|r|r.kind!="node"){
   if let Some(core)=cores.iter().min_by(|a,b|a.center().distance(resource.pos).total_cmp(&b.center().distance(resource.pos))){road(&mut game.state.terrain,core.center(),resource.pos);}
  }
  // A resource marker never leaves an unmatched rock or highland at its mirror position.
  for resource in &game.state.resources{for x in[resource.pos.x,WIDTH-1-resource.pos.x]{for y in[resource.pos.y,HEIGHT-1-resource.pos.y]{let index=(y*WIDTH+x)as usize;if matches!(game.state.terrain[index],1|2|4){game.state.terrain[index]=0;}}}}
  // Resource markers keep their real type. These traversable labels do not alter access costs.
  for resource in &game.state.resources{let terrain=match resource.kind.as_str(){"ore"=>5,"coal"=>6,_=>3};set(&mut game.state.terrain,resource.pos.x,resource.pos.y,terrain);}
  game.refresh_fog();game
 }
}

#[cfg(test)]mod tests{
 use super::*;
 #[test]fn pads_follow_real_core_footprints(){for theme in["river","mining","highland"]{for seed in 1..=20{let g=Game::new_theme(seed,false,theme);for core in g.state.buildings.iter().filter(|b|b.kind=="core"){for y in core.rect.y-10..core.rect.y+core.rect.height+10{for x in core.rect.x-8..core.rect.x+core.rect.width+8{assert!(!matches!(g.terrain(Pos::new(x,y,0)),1|2),"{theme}/{seed}: ({x},{y})");}}}}}}
 #[test]fn obstruction_and_highland_patterns_are_mirrored(){for theme in["river","mining","highland"]{let g=Game::new_theme(81,false,theme);let class=|n:u8|if matches!(n,3|5|6){0}else{n};for y in 0..HEIGHT{for x in 0..WIDTH{assert_eq!(class(g.terrain(Pos::new(x,y,0))),class(g.terrain(Pos::new(WIDTH-1-x,y,0))),"{theme} ({x},{y})");}}}}
 #[test]fn changing_theme_preserves_seed_core_and_resource_identity(){let a=Game::new_theme(19,false,"river");let b=Game::new_theme(19,false,"highland");assert_eq!(a.state.seed,b.state.seed);assert_eq!(a.state.buildings[0].rect,b.state.buildings[0].rect);assert_eq!(a.state.resources.iter().map(|r|r.id).collect::<Vec<_>>(),b.state.resources.iter().map(|r|r.id).collect::<Vec<_>>());assert_ne!(a.state.terrain,b.state.terrain);}
}
