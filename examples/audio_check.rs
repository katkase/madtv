//! Verifica che l'apertura dell'audio non si blocchi e dica cosa ha trovato.
fn main() {
    match madtv::audio::AudioOut::open() {
        Some(a) => println!("dispositivo: {} a {} Hz", a.device_name, a.rate),
        None => println!("nessun dispositivo audio: il gioco partirebbe senza suono"),
    }
}
