/// The 44 bytes that precede PCM samples in a canonical WAV file.
pub(super) fn header(rate: u32, channels: u16, bits: u16, data_len: u32) -> [u8; 44] {
    let block = channels * bits / 8;
    let mut out = [0u8; 44];
    let mut put = |at: usize, bytes: &[u8]| out[at..at + bytes.len()].copy_from_slice(bytes);
    put(0, b"RIFF");
    put(4, &(36 + data_len).to_le_bytes());
    put(8, b"WAVEfmt ");
    put(16, &16u32.to_le_bytes());
    put(20, &1u16.to_le_bytes()); // PCM
    put(22, &channels.to_le_bytes());
    put(24, &rate.to_le_bytes());
    put(28, &(rate * u32::from(block)).to_le_bytes());
    put(32, &block.to_le_bytes());
    put(34, &bits.to_le_bytes());
    put(36, b"data");
    put(40, &data_len.to_le_bytes());
    out
}
