use bevy_callback::prelude::*;
use bevy_app::{
    App,
    Startup, Update,
    ScheduleRunnerPlugin
};
use bevy_ecs::{
    component::Component,
    event::Event,
    query::With,
    schedule::ApplyDeferred,
    system::{
        Query,
        Commands
    }
};


#[derive(Component)]
struct Player {
    name   : String,
    pinged : usize
}

#[derive(Event)]
struct StatusRequest;


fn main() {
    App::new()
        .add_plugins(ScheduleRunnerPlugin::run_once())
        .add_callback(status_response)
        .add_systems(Startup, spawn_players)
        .add_systems(Update, (
            request_status,
            ApplyDeferred
        ))
        .add_systems(Update, (
            request_status,
            ApplyDeferred
        ))
        .add_systems(Update, (
            request_status,
            ApplyDeferred
        ))
        .add_systems(Update, (
            request_status,
            ApplyDeferred
        ))
        .add_systems(Update, (
            request_status,
            ApplyDeferred
        ))
        .run();
}


fn spawn_players(
    mut cmds : Commands
) {
    cmds.spawn(Player { name : "A".to_string(), pinged : 0 });
    cmds.spawn(Player { name : "B".to_string(), pinged : 0 });
    cmds.spawn(Player { name : "C".to_string(), pinged : 0 });
    cmds.spawn(Player { name : "D".to_string(), pinged : 0 });
}


fn request_status(
    mut query    : Query<&mut Player>,
    mut callback : Callback<StatusRequest, String>
) {
    println!("Current status: {:?}", callback.request(StatusRequest));
    println!("Expected {}", query.iter().len());
    for mut player in &mut query {
        println!("{} says hi ({})", player.name, player.pinged);
        player.pinged += 1;
    }
}


fn status_response(
        _request : Request<StatusRequest>,
    mut cmds     : Commands,
        query    : Query<(), With<Player>>
) -> String {
    cmds.spawn(Player { name : "new".to_string(), pinged : 0 });
    format!("{} players exist.", query.iter().len())
}
