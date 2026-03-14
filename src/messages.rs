use actix::Message;

pub struct InitialData(pub String);

impl Message for InitialData {
    type Result = ();
}
