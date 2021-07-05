use wayc::*;

#[test]
fn connect_to_server() -> Result<(), anyhow::Error> {
    let mut wc = Wayc::new()?;

    wc.dispatch();
    Ok(())
}

#[test]
fn create_shm_buf() -> Result<(), anyhow::Error> {
    let mut wc = Wayc::new()?;
    let surf = wc.create_surface()?;
    let buf = wc.create_shm_buffer(640, 480)?;

    wc.dispatch();
    wc.flush();
    Ok(())
}
