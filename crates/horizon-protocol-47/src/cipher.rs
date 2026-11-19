use aes::{
    Aes128,
    cipher::{BlockEncrypt, KeyInit, generic_array::GenericArray},
};

/// One direction of the continuous AES/CFB8 stream. Minecraft uses key = IV.
pub(crate) struct Cipher {
    aes: Aes128,
    register: [u8; 16],
}

impl Cipher {
    pub(crate) fn new(secret: &[u8; 16]) -> Self {
        Self {
            aes: Aes128::new(GenericArray::from_slice(secret)),
            register: *secret,
        }
    }

    pub(crate) fn apply(&mut self, bytes: &mut [u8], encrypt: bool) {
        for byte in bytes {
            let mut block = GenericArray::clone_from_slice(&self.register);
            self.aes.encrypt_block(&mut block);
            let ciphertext = *byte;
            *byte ^= block[0];
            self.register.copy_within(1.., 0);
            self.register[15] = if encrypt { *byte } else { ciphertext };
        }
    }
}
