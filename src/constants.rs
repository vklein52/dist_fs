use crate::heartbeat::Timestamp;

// House
pub const IP_LIST: [&str; 4] = [
    "192.168.86.70:9000",
    "192.168.86.70:9001",
    "192.168.86.70:9002",
    "192.168.86.68:9000",
    // when youre not lazy, get a setup of the machines working
];

// Apartment
// pub const IP_LIST: [&str; 4] = [
//     "192.168.0.124:9000",
//     "192.168.0.124:9001",
//     "192.168.0.124:9002",
//     "192.168.0.174:9000",
//     // when youre not lazy, get a setup of the machines working
// ];

pub static HEADER_SIZE: usize = 5;
pub static NUM_SUCCESSORS: u32 = 2;
pub static EXPIRATION_DURATION: Timestamp = 3;

// pub const IP_LIST: [&str; 4] = [
//     "localhost:9000",
//     "localhost:9001",
//     "localhost:9002",
//     "localhost:9003",
//     // when youre not lazy, get a setup of the machines working
// ];
