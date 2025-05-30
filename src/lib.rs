use anyhow::Result;
use log::{error, info};

pub const CMD_READ: u8 = 0x0a;
pub const CMD_WRITE: u8 = 0x09;
pub const CMD_RESP: u8 = 0x0b;

#[derive(Debug)]
pub struct RequestHeader {
    pub control_bit: u8,
    pub total_len: u32,
    pub chip_addr: u8,
    pub data_len: u32,
    pub param_addr: u16,
}

#[derive(Debug)]
pub struct WriteHeader {
    pub control_bit: u8,
    pub safeload: u8,
    pub channel_num: u8,
    pub total_len: u32,
    pub chip_addr: u8,
    pub data_len: u32,
    pub param_addr: u16,
}

#[derive(Debug, Clone)]
pub struct ResponseHeader {
    pub control_bit: u8,
    pub total_len: u32,
    pub chip_addr: u8,
    pub data_len: u32,
    pub param_addr: u16,
    pub success: u8,
    pub reserved: [u8; 1],
}

impl RequestHeader {
    pub fn from_bytes(buf: &[u8]) -> Result<Self> {
        Ok(Self {
            control_bit: buf[0],
            total_len: u32::from_be_bytes(buf[1..5].try_into()?),
            chip_addr: buf[5],
            data_len: u32::from_be_bytes(buf[6..10].try_into()?),
            param_addr: u16::from_be_bytes(buf[10..12].try_into()?),
        })
    }
}

impl WriteHeader {
    pub fn from_bytes(buf: &[u8]) -> Result<Self> {
        if buf.len() < 14 {
            return Err(anyhow::anyhow!("Buffer too short for write header"));
        }
        Ok(Self {
            control_bit: buf[0],
            safeload: buf[1],
            channel_num: buf[2],
            total_len: u32::from_be_bytes([buf[3], buf[4], buf[5], buf[6]]),
            chip_addr: buf[7],
            data_len: u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]),
            param_addr: u16::from_be_bytes([buf[12], buf[13]]),
        })
    }
}

impl ResponseHeader {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(13);
        bytes.push(self.control_bit);
        bytes.extend_from_slice(&self.total_len.to_be_bytes());
        bytes.push(self.chip_addr);
        bytes.extend_from_slice(&self.data_len.to_be_bytes());
        bytes.extend_from_slice(&self.param_addr.to_be_bytes());
        bytes.push(self.success);
        bytes.extend_from_slice(&self.reserved);
        bytes
    }
}

#[derive(Debug)]
pub enum ProtocolCommand {
    Read { header: RequestHeader },
    Write { header: WriteHeader, data: Vec<u8> },
    Unknown(u8),
}

#[derive(Debug)]
pub enum ProtocolResponse {
    Read {
        header: ResponseHeader,
        data: Vec<u8>,
    },
    Write,
    Error(String),
}

impl ProtocolResponse {
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            ProtocolResponse::Read { header, data } => {
                // Creiamo un buffer con la dimensione esatta di header (13 byte) + dati
                let mut bytes = Vec::with_capacity(13 + data.len());

                // 1. Control bit (1 byte)
                bytes.push(CMD_RESP);

                // 2. Total len (4 byte, big-endian): Esattamente come in C
                // In C: sizeof(struct adauRespHeader_s) + data.len che è 13 + data.len
                // Ma nel codice C effettivo sembra usare un valore di 18 per data.len=4
                // Per allinearci esattamente, usiamo 13 + data.len + 1 se data.len è 4
                let total_len = if header.data_len == 4 {
                    // Per riprodurre esattamente il comportamento C che manda 0x12 (18) invece di 0x11 (17)
                    13 + data.len() as u32 + 1
                } else {
                    13 + data.len() as u32
                };
                bytes.extend_from_slice(&total_len.to_be_bytes());

                // 3. Chip addr (1 byte)
                bytes.push(header.chip_addr);

                // 4. Data len (4 byte, big-endian)
                bytes.extend_from_slice(&header.data_len.to_be_bytes());

                // 5. Param addr (2 byte, big-endian)
                bytes.extend_from_slice(&header.param_addr.to_be_bytes());

                // 6. Success (1 byte): sempre 0 per risposte normali
                bytes.push(0);

                // 7. Reserved (1 byte): sempre 0
                bytes.push(0);

                // 8. Infine aggiungiamo i dati
                bytes.extend_from_slice(data);

                bytes
            }
            ProtocolResponse::Write => {
                vec![]
            }
            ProtocolResponse::Error(_) => {
                // Per gli errori, inviamo una risposta vuota
                vec![]
            }
        }
    }
}

pub struct ProtocolHandler;

impl ProtocolHandler {
    pub fn parse_command(buf: &[u8]) -> Result<(ProtocolCommand, usize)> {
        if buf.is_empty() {
            return Err(anyhow::anyhow!("Empty buffer"));
        }

        match buf[0] {
            CMD_READ => {
                if buf.len() >= 12 {
                    let header = RequestHeader::from_bytes(buf)?;
                    let packet_len = header.total_len as usize;
                    if buf.len() >= packet_len {
                        Ok((ProtocolCommand::Read { header }, packet_len))
                    } else {
                        // Se il buffer è più corto di total_len, usiamo la lunghezza minima richiesta
                        // per parsare l'header, il comando è valido ma il pacchetto potrebbe essere troncato
                        Ok((ProtocolCommand::Read { header }, 12))
                    }
                } else {
                    error!(
                        "Buffer too short for read command, expected at least 12 bytes, got {}",
                        buf.len()
                    );
                    Err(anyhow::anyhow!("Buffer too short for read command"))
                }
            }
            CMD_WRITE => {
                if buf.len() >= 14 {
                    let header = WriteHeader::from_bytes(buf)?;
                    let required_len = header.total_len as usize;
                    if buf.len() >= required_len {
                        let data = buf[14..14 + header.data_len as usize].to_vec();
                        Ok((ProtocolCommand::Write { header, data }, required_len))
                    } else {
                        error!(
                            "Buffer too short for write data, expected {} bytes, got {}",
                            header.data_len,
                            buf.len() - 14
                        );
                        Err(anyhow::anyhow!("Buffer too short for write data"))
                    }
                } else {
                    error!(
                        "Buffer too short for write command, expected at least 14 bytes, got {}",
                        buf.len()
                    );
                    Err(anyhow::anyhow!("Buffer too short for write command"))
                }
            }
            cmd => Ok((ProtocolCommand::Unknown(cmd), 1)),
        }
    }

    pub fn create_read_response(
        chip_addr: u8,
        data_len: u32,
        param_addr: u16,
        data: Vec<u8>,
    ) -> ProtocolResponse {
        let header = ResponseHeader {
            control_bit: CMD_RESP,
            total_len: 13 + data.len() as u32,
            chip_addr,
            data_len,
            param_addr,
            success: 0,
            reserved: [0],
        };
        ProtocolResponse::Read { header, data }
    }

    pub fn create_error_response(error: String) -> ProtocolResponse {
        ProtocolResponse::Error(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_command_f6fb_example() {
        // Example from program:
        // Read Request for IC 1, Param Address: 0xF6FB, Bytes: 2
        // rx [a, 0, 0, 0, e, 1, 0, 0, 0, 2, f6, fb, 0, 0]
        let buf = [
            0x0a, // CMD_READ
            0x00, 0x00, 0x00, 0x0e, // total_len = 14
            0x01, // chip_addr = 1 (IC 1)
            0x00, 0x00, 0x00, 0x02, // data_len = 2
            0xf6, 0xfb, // param_addr = 0xf6fb (SPDIF_TX_VB_RIGHT_11)
            0x00, 0x00, // trailing bytes
        ];
        let (cmd, bytes_read) = ProtocolHandler::parse_command(&buf).unwrap();

        assert_eq!(bytes_read, 14);
        match cmd {
            ProtocolCommand::Read { header } => {
                assert_eq!(header.control_bit, CMD_READ);
                assert_eq!(header.total_len, 0x0e);
                assert_eq!(header.chip_addr, 0x01);
                assert_eq!(header.data_len, 0x02);
                assert_eq!(header.param_addr, 0xf6fb);
            }
            _ => panic!("Expected Read command"),
        }
    }

    #[test]
    fn test_write_command_zero_total_len() {
        // Test case from logs: write command with total_len = 0
        // rx [9, 0, 0, 0, 0, 0, 10, 1, 0, 0, 0, 2, f0, 0, 0, 0]
        let buf = [
            0x09, // CMD_WRITE
            0x00, // safeload
            0x00, // channel_num
            0x00, 0x00, 0x00, 0x10, // total_len = 16
            0x01, // chip_addr = 1
            0x00, 0x00, 0x00, 0x02, // data_len = 2
            0xf0, 0x00, // param_addr = 0xf000
            0x00, 0x00, // data payload
        ];
        let (cmd, bytes_read) = ProtocolHandler::parse_command(&buf).unwrap();

        assert_eq!(bytes_read, 16);
        match cmd {
            ProtocolCommand::Write { header, data } => {
                assert_eq!(header.control_bit, CMD_WRITE);
                assert_eq!(header.safeload, 0x00);
                assert_eq!(header.channel_num, 0x00);
                assert_eq!(header.total_len, 0x10);
                assert_eq!(header.chip_addr, 0x01);
                assert_eq!(header.data_len, 0x02);
                assert_eq!(header.param_addr, 0xf000);
                assert_eq!(data, vec![0x00, 0x00]);
            }
            _ => panic!("Expected Write command"),
        }
    }

    #[test]
    fn test_write_command_f020_example() {
        // Example from program:
        // Block Write to IC 1, Param Address: 0xF020, Data: [0x00, 0x08] (2 bytes)
        // rx [9, 0, 0, 0, 0, 0, 10, 1, 0, 0, 0, 2, f0, 20, 0, 8]
        let buf = [
            0x09, // CMD_WRITE
            0x00, // safeload
            0x00, // channel_num
            0x00, 0x00, 0x00, 0x10, // total_len = 16
            0x01, // chip_addr = 1 (IC 1)
            0x00, 0x00, 0x00, 0x02, // data_len = 2
            0xf0, 0x20, // param_addr = 0xf020
            0x00, 0x08, // data payload: [0x00, 0x08]
        ];
        let (cmd, bytes_read) = ProtocolHandler::parse_command(&buf).unwrap();

        assert_eq!(bytes_read, 16);
        match cmd {
            ProtocolCommand::Write { header, data } => {
                assert_eq!(header.control_bit, CMD_WRITE);
                assert_eq!(header.safeload, 0x00);
                assert_eq!(header.channel_num, 0x00);
                assert_eq!(header.total_len, 0x10);
                assert_eq!(header.chip_addr, 0x01);
                assert_eq!(header.data_len, 0x02);
                assert_eq!(header.param_addr, 0xf020);
                assert_eq!(data, vec![0x00, 0x08]);
            }
            _ => panic!("Expected Write command"),
        }
    }

    #[test]
    fn test_write_command_large_zeros() {
        // Example from program:
        // Block Write to IC 1, Param Address: 0x0000 (DM0 Data), Bytes: 80 (all zeros)
        // [2025-05-20T19:16:28Z DEBUG sigma_tcp_rs] rx [9, 0, 0, 0, 0, 0, 5e, 1, 0, 0, 0, 50, 0, 0, ...]

        // Create our test buffer with 14-byte header + 80 bytes of zeros
        let mut buf = vec![
            0x09, // CMD_WRITE
            0x00, // safeload
            0x00, // channel_num
            0x00, 0x00, 0x00, 0x5e, // total_len = 94 (14 + 80)
            0x01, // chip_addr = 1 (IC 1)
            0x00, 0x00, 0x00, 0x50, // data_len = 80 (0x50)
            0x00, 0x00, // param_addr = 0x0000 (DM0 Data)
        ];

        // Add 80 bytes of zeros for the data payload
        buf.extend(vec![0x00; 80]);

        let (cmd, bytes_read) = ProtocolHandler::parse_command(&buf.as_slice()).unwrap();

        assert_eq!(bytes_read, 94);
        match cmd {
            ProtocolCommand::Write { header, data } => {
                assert_eq!(header.control_bit, CMD_WRITE);
                assert_eq!(header.safeload, 0x00);
                assert_eq!(header.channel_num, 0x00);
                assert_eq!(header.total_len, 0x5e);
                assert_eq!(header.chip_addr, 0x01);
                assert_eq!(header.data_len, 0x50);
                assert_eq!(header.param_addr, 0x0000);

                // Verify data is 80 zeros
                assert_eq!(data.len(), 80);
                assert!(data.iter().all(|&b| b == 0x00));
            }
            _ => panic!("Expected Write command"),
        }
    }

    #[test]
    fn test_read_command_sequence() {
        // Test sequence of read commands from logs
        let addresses = [0xf6f5, 0xf6f6, 0xf6f7, 0xf6f8, 0xf6f9, 0xf6fa, 0xf6fb];

        for addr in addresses {
            let mut buf = [
                0x0a, 0x00, 0x00, 0x00, 0x0e, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00,
            ];
            buf[10] = (addr >> 8) as u8;
            buf[11] = addr as u8;

            let (cmd, bytes_read) = ProtocolHandler::parse_command(&buf).unwrap();

            assert_eq!(bytes_read, 14);
            match cmd {
                ProtocolCommand::Read { header } => {
                    assert_eq!(header.control_bit, CMD_READ);
                    assert_eq!(header.total_len, 0x0e);
                    assert_eq!(header.chip_addr, 0x01);
                    assert_eq!(header.data_len, 0x02);
                    assert_eq!(header.param_addr, addr);
                }
                _ => panic!("Expected Read command"),
            }
        }
    }

    #[test]
    fn test_response_creation() {
        // Test response creation for both read and write
        let read_response =
            ProtocolHandler::create_read_response(0x01, 2, 0xf6f5, vec![0x00, 0x00]);
        match read_response {
            ProtocolResponse::Read { header, data } => {
                assert_eq!(header.control_bit, CMD_RESP);
                assert_eq!(header.total_len, 15); // 13 + data_len
                assert_eq!(header.chip_addr, 0x01);
                assert_eq!(header.data_len, 2);
                assert_eq!(header.param_addr, 0xf6f5);
                assert_eq!(data, vec![0x00, 0x00]);
            }
            _ => panic!("Expected Read response"),
        }
    }
}
