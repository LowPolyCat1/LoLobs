use crate::lcu::get_initial_rank;
use crate::messages::InitialData;
use actix::{Actor, Addr, AsyncContext, Handler, StreamHandler};
use actix_web_actors::ws;
use tokio::sync::broadcast;

pub struct MyWs {
    pub receiver: broadcast::Receiver<String>,
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

        let addr = ctx.address();
        ctx.run_later(std::time::Duration::from_millis(100), move |_, _| {
            tokio::spawn(async move {
                if let Some(data) = get_initial_rank().await {
                    addr.do_send(InitialData(data));
                }
                if let Some(data) = crate::lcu::get_summoner_data().await {
                    addr.do_send(InitialData(data));
                }
            });
        });
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
        if let Ok(ws::Message::Ping(msg)) = msg {
            ctx.pong(&msg);
        }
    }
}
