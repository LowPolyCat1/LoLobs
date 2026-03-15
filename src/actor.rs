use crate::lcu::fetch_endpoint;
use crate::messages::InitialData;
use actix::{Actor, Addr, AsyncContext, Handler, StreamHandler};
use actix_web_actors::ws;
use tokio::sync::broadcast;

pub struct MyWs {
    pub receiver: broadcast::Receiver<String>,
}

impl MyWs {
    fn send_initial_state(addr: Addr<Self>) {
        tokio::spawn(async move {
            let ranked = fetch_endpoint("/lol-ranked/v1/current-ranked-stats").await;
            let summoner = fetch_endpoint("/lol-summoner/v1/current-summoner").await;

            if let Some(r) = ranked {
                addr.do_send(InitialData(r.to_string()));
            }
            if let Some(s) = summoner {
                addr.do_send(InitialData(s.to_string()));
            }
        });
    }
}

impl Actor for MyWs {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let mut rx = self.receiver.resubscribe();

        ctx.add_stream(async_stream::stream! {
            while let Ok(msg) = rx.recv().await {
                yield msg;
            }
        });

        Self::send_initial_state(ctx.address());
    }
}

impl Handler<InitialData> for MyWs {
    type Result = ();
    fn handle(&mut self, msg: InitialData, ctx: &mut Self::Context) {
        ctx.text(msg.0);
    }
}

impl StreamHandler<String> for MyWs {
    fn handle(&mut self, msg: String, ctx: &mut Self::Context) {
        ctx.text(msg);
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for MyWs {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        if let Ok(ws::Message::Ping(bytes)) = msg {
            ctx.pong(&bytes);
        }
    }
}
