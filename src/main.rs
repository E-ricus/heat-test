use heat_test::Controller;

#[tokio::main]
async fn main() {
    match Controller::new() {
        Ok(controller) => controller.controll().await,
        Err(e) => eprintln!("Error creating controller {e}"),
    }
}
