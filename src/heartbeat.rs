use crate::BoxedErrorResult;
use crate::component_manager::*;
use crate::constants;
use crate::filesystem;
use crate::globals;
use crate::modular::*;
use crate::operation::*;
use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::iter::FromIterator;
use std::time::SystemTime;

type HeartBeatResult = BoxedErrorResult<()>;
pub type Timestamp = u64;

pub fn maintainer(sender: &OperationSender) -> ComponentResult {
    if !is_joined() {
        return Ok(())
    }
    // Figure out who is expired
    let mut expired_nodes: Vec<String> = Vec::new();
    for predecessor in globals::PREDECESSOR_LIST.read().iter() {
        if is_expired(&predecessor)? {
            expired_nodes.push(predecessor.clone());
        }
    }
    if expired_nodes.len() > 0 {
        // Remove the expired lads and let everyone know
        for expired_node in expired_nodes.iter() {
            log(format!("Detected a failure on node {}", &expired_node));
            let leave_operation = LeaveOperation{
                id: expired_node.clone()
            };
            let mut generated_operations = leave_operation.execute(Source::myself())?;
            for generated_operation in generated_operations.drain(..) {
                sender.send(generated_operation)?;
            }
        }
    }
    Ok(())
}

pub fn join(_args: Vec<&str>, sender: &OperationSender) -> HeartBeatResult {
    if is_joined() {
        println!("Already joined");
        return Ok(());
    }
    // Mark self as joined and in membership list
    globals::IS_JOINED.write(true);
    let my_id = gen_id()?;
    globals::MY_ID.write(my_id.clone());
    globals::MEMBERSHIP_LIST.get_mut().push(my_id.clone());
    globals::UDP_TO_TCP_MAP.get_mut().insert(ip_from_id(&my_id), globals::TCP_ADDR.read().clone());
    // Send Join operation to everyone
    let dests = constants::IP_LIST.iter().map(|x| x.to_string()).collect();
    let join_item = SendableOperation {
        dests: Destinations::UDPAddr(dests),
        operation: Box::new(JoinOperation {
            id: my_id.clone(),
            tcp_addr: globals::TCP_ADDR.read().clone()
        })
    };
    sender.send(join_item)?;
    log(format!("Joined the network with id: {}", my_id));
    Ok(())
}

pub fn leave(_args: Vec<&str>, sender: &OperationSender) -> HeartBeatResult {
    check_joined()?;
    // Send the leave operation and clear vars
    let my_id = globals::MY_ID.read().clone();
    let leave_item = SendableOperation::for_successors(Box::new(LeaveOperation{
        id: my_id.clone()
    }));
    sender.send(leave_item)?;
    clear_vars_on_leave();
    log(format!("Left the network with id: {}", my_id));
    Ok(())
}

pub fn print(_args: Vec<&str>) -> HeartBeatResult {
    let list = globals::MEMBERSHIP_LIST.read();
    println!("[");
    for memb in list.iter() {
        println!("  {}", memb);
    }
    println!("]");
    Ok(())
}

pub fn send_heartbeats() -> HeartBeatResult {
    let udp_socket = globals::UDP_SOCKET.read();
    let successor_list = globals::SUCCESSOR_LIST.read().clone();
    let heartbeat_operation = Box::new(HeartbeatOperation {
        id: globals::MY_ID.read().clone()
    });
    let heartbeats = SendableOperation::for_successors(heartbeat_operation);
    heartbeats.write_all_udp(&udp_socket)
}

// Helpers
pub fn is_master() -> bool {
    // TODO: Eliminate the need for a master in the final product
    let membership_list = globals::MEMBERSHIP_LIST.read();
    membership_list.len() > 0 && membership_list[0] == *globals::MY_ID.read()
}

pub fn ips_from_ids(ids: &Vec<String>) -> Vec<String> {
    ids.iter().map(|x| {
        ip_from_id(x)
    }).collect()
}

pub fn ip_from_id(id: &String) -> String {
    let n = id.find('|').unwrap();
    String::from(&id[..n])    
}

pub fn tcp_ips_from_udp_ips(udp_ips: &Vec<String>) -> BoxedErrorResult<Vec<String>> {
    let tcp_map = globals::UDP_TO_TCP_MAP.read();
    let mut output = Vec::new();
    for udp_ip in udp_ips.iter() {
        output.push(tcp_map.get(udp_ip)
                    .ok_or(format!("No TCP addr entry for destination {}", udp_ip))?.to_string());
    }
    Ok(output)
}

pub fn tcp_ips_from_ids(ids: &Vec<String>) -> BoxedErrorResult<Vec<String>> {
    let udp_ips = ips_from_ids(&ids);
    tcp_ips_from_udp_ips(&udp_ips)
}

pub fn insert_node(new_id: &String) -> HeartBeatResult {
    let mut membership_list = globals::MEMBERSHIP_LIST.get_mut();
    if let Err(idx) = membership_list.binary_search(&new_id) {
        membership_list.insert(idx, new_id.to_string());
    }
    Ok(())
}

pub fn remove_node(id: &String) -> BoxedErrorResult<bool> {
    let mut membership_list = globals::MEMBERSHIP_LIST.get_mut();
    match membership_list.binary_search(&id).clone() {
        Ok(idx) => {
            membership_list.remove(idx);
            Ok(true)
        }
        Err(_) => Ok(false)
    }
}

fn clear_vars_on_leave() -> BoxedErrorResult<()> {
    globals::IS_JOINED.write(false);
    globals::MEMBERSHIP_LIST.get_mut().clear();
    globals::SUCCESSOR_LIST.get_mut().clear();
    globals::PREDECESSOR_LIST.get_mut().clear();
    // Does keeping the old id matter? If so, edit the locks to be able to write None back in
    Ok(())
}

pub fn merge_membership_list(membership_list: &Vec<String>) -> HeartBeatResult {
    // TODO: Optimize?
    for member_id in membership_list {
        insert_node(member_id)?;
    }
    Ok(())
}

pub fn merge_tcp_map(udp_to_tcp_map: &HashMap<String, String>) -> HeartBeatResult {
    globals::UDP_TO_TCP_MAP.get_mut().extend(udp_to_tcp_map.clone());
    Ok(())
}

pub fn merge_all_file_owners(new_file_owners: &HashMap<String, HashSet<String>>) -> HeartBeatResult {
    let mut all_file_owners = globals::ALL_FILE_OWNERS.get_mut();
    for (filename, new_owners) in new_file_owners.iter() {
        let mut file_owners = all_file_owners.entry(filename.to_string()).or_insert(HashSet::new());
        // TODO: Another place to reduce clones/allocations -> Cows could help here and in a lot of other places
        *file_owners = file_owners.union(new_owners).map(|x| x.to_string()).collect();
    }
    Ok(())
}

pub fn recalculate_neighbors() -> HeartBeatResult {
    recalculate_predecessors()?;
    recalculate_successors()?;
    log(format!("Membership list is {:?}", &*globals::MEMBERSHIP_LIST.read()));
    log(format!("TCP map is {:?}", &*globals::UDP_TO_TCP_MAP.read()));
    Ok(())
}

fn recalculate_successors() -> HeartBeatResult {
    let successors = gen_neighbor_list(1)?;
    log(format!("Calculated successors as {:?}", successors));
    globals::SUCCESSOR_LIST.write(successors);
    Ok(())
}

fn recalculate_predecessors() -> HeartBeatResult {
    // Get the new predecessors
    let new_predecessors = gen_neighbor_list(-1)?;
    log(format!("Calculated predecessors as {:?}", new_predecessors));
    // Save the old predecessors
    let mut predecessors = globals::PREDECESSOR_LIST.get_mut();
    // For each deleted, remove from the timestamp dict
    let mut predecessor_timestamps = globals::PREDECESSOR_TIMESTAMPS.get_mut();
    let old_predecessors_set: HashSet<&String> = HashSet::from_iter(predecessors.iter());
    let new_predecessors_set: HashSet<&String> = HashSet::from_iter(new_predecessors.iter());
    let deleted_predecessors = &old_predecessors_set - &new_predecessors_set;
    for deleted_predecessor in deleted_predecessors.iter() {
        predecessor_timestamps.remove(&deleted_predecessor.to_string());
    }
    // For each new guy, add to the timestamp dict
    let inserted_predecessors = &new_predecessors_set - &old_predecessors_set;
    for inserted_predecessor in inserted_predecessors.iter() {
        predecessor_timestamps.insert(inserted_predecessor.to_string(), get_timestamp()?);
    }
    // Write the new predecessors
    *predecessors = new_predecessors;
    Ok(())
}

// TODO: Eventually change this code to make sure the data fits
fn gen_neighbor_list(increment: i32) -> BoxedErrorResult<Vec<String>> {
    // Ensure the membership_list has members
    let membership_list = globals::MEMBERSHIP_LIST.read();
    let len = membership_list.len();
    if len == 0 {
        return Err("Cannot calculate successors/predecessors because membership list is empty".into())
    }
    // Traverse modulo membership list size
    let my_idx: usize = membership_list.binary_search(&*globals::MY_ID.read())
        .expect("ID of node not found in membership list");
    gen_neighbor_list_from(my_idx as i32, increment, constants::NUM_SUCCESSORS, false)
}

pub fn gen_neighbor_list_from(idx: i32, increment: i32, num_successors: u32, include_self: bool) -> BoxedErrorResult<Vec<String>> {
    let membership_list = globals::MEMBERSHIP_LIST.read();
    let len = membership_list.len();
    let start_idx = Modular::new(idx + increment, len as u32);
    let mut new_neighbors = Vec::new();
    for i in 0..num_successors {
        let curr_idx = start_idx.clone() + increment * i as i32;
        if *curr_idx == idx as u32 {
            if include_self {
                new_neighbors.push(membership_list[*curr_idx as usize].clone());
            }
            break
        }
        new_neighbors.push(membership_list[*curr_idx as usize].clone());
    }
    Ok(new_neighbors)
    
}

fn is_expired(id: &String) -> BoxedErrorResult<bool> {
    let now = get_timestamp()?;
    match globals::PREDECESSOR_TIMESTAMPS.read().get(&id.to_string()) {
        Some(last_timestamp) => {
            if now < *last_timestamp {
                return Err("Invalid timestamp found in predecessor timestamps".into())
            }
            Ok(now - last_timestamp > constants::EXPIRATION_DURATION)
        }
        None => Err("Tried to check if non-predecessor node is expired".into())
    }
}

fn gen_id() -> BoxedErrorResult<String> {
    Ok(format!("{}|{}", *globals::MY_IP_ADDR.read(), get_timestamp()?).to_string())
}

pub fn get_timestamp() -> BoxedErrorResult<Timestamp> {
    Ok(SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?.as_secs())
}

// Operations
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HeartbeatOperation {
    pub id: String
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JoinOperation {
    pub id: String,
    pub tcp_addr: String 
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LeaveOperation {
    pub id: String
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NewMemberOperation {
    pub id: String,
    pub tcp_addr: String 
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MemberInitializationOperation {
    membership_list: Vec<String>,
    udp_to_tcp_map: HashMap<String, String>,
    all_file_owners: HashMap<String, HashSet<String>>
}

// Trait Impls
impl OperationWriteExecute for HeartbeatOperation {
    fn to_bytes(&self) -> BoxedErrorResult<Vec<u8>> {
        Ok(create_buf(&self, str_to_vec("HB  ")))
    }
    fn execute(&self, source: Source) -> BoxedErrorResult<Vec<SendableOperation>> {
        // Assert that source and self.id correspond to same ip
        let src_ip = ip_from_id(&self.id);
        if src_ip != TryInto::<String>::try_into(source)? {
            return Err("Received suspicious heartbeat packet with source != id".into())
        }
        // Update the most recent observation if this is a predecessor
        if let Some(predecessor_entry) = globals::PREDECESSOR_TIMESTAMPS.get_mut().get_mut(&self.id) {
            *predecessor_entry = get_timestamp()?;
        }
        // TODO: Maybe add user back in case of accidental leave packet - may need more synchronization
        Ok(vec![])
    }
    fn to_string(&self) -> String { format!("{:?}", self) }
}

impl OperationWriteExecute for JoinOperation {
    fn to_bytes(&self) -> BoxedErrorResult<Vec<u8>> {
        Ok(create_buf(&self, str_to_vec("JOIN")))
    }
    fn execute(&self, source: Source) -> BoxedErrorResult<Vec<SendableOperation>> {
        // Add the new guy and send it to everyone
        let mut generated_operations: Vec<SendableOperation> = Vec::new();
        insert_node(&self.id)?;
        globals::UDP_TO_TCP_MAP.get_mut().insert(TryInto::<String>::try_into(source)?, self.tcp_addr.clone());
        generated_operations.push(
            SendableOperation::for_everyone(Box::new(NewMemberOperation{
                id: self.id.clone(),
                tcp_addr: self.tcp_addr.clone()
            }))
        );
        generated_operations.push(
            SendableOperation::for_single(self.id.to_string(), Box::new(MemberInitializationOperation{
                membership_list: globals::MEMBERSHIP_LIST.read().clone(),
                udp_to_tcp_map: globals::UDP_TO_TCP_MAP.read().clone(),
                all_file_owners: globals::ALL_FILE_OWNERS.read().clone()
            }))
        );
        recalculate_neighbors()?;
        Ok(generated_operations)
    }
    fn to_string(&self) -> String { format!("{:?}", self) }
}


impl OperationWriteExecute for LeaveOperation {
    fn to_bytes(&self) -> BoxedErrorResult<Vec<u8>> {
        Ok(create_buf(&self, str_to_vec("LEAV")))
    }
    fn execute(&self, _source: Source) -> BoxedErrorResult<Vec<SendableOperation>> {
        let mut generated_operations: Vec<SendableOperation> = Vec::new();
        let removed = remove_node(&self.id)?;
        if removed {
            globals::UDP_TO_TCP_MAP.get_mut().remove(&self.id);
            generated_operations.push(SendableOperation::for_successors(Box::new(self.clone())));
            recalculate_neighbors()?;
            log(format!("Started handling failed node {}", &self.id));
            generated_operations.append(&mut filesystem::handle_failed_node(&self.id)?);
            log(format!("Finished handling failed node {}", &self.id));
        }
        Ok(generated_operations)
    }
    fn to_string(&self) -> String { format!("{:?}", self) }
}

impl OperationWriteExecute for NewMemberOperation {
    fn to_bytes(&self) -> BoxedErrorResult<Vec<u8>> {
        Ok(create_buf(&self, str_to_vec("NMEM")))
    }
    fn execute(&self, _source: Source) -> BoxedErrorResult<Vec<SendableOperation>> {
        insert_node(&self.id)?;
        globals::UDP_TO_TCP_MAP.get_mut().insert(ip_from_id(&self.id), self.tcp_addr.clone());
        // MAYBE TODO: Do we need more redundancy to make sure joins are not missed?
        recalculate_neighbors()?;
        Ok(vec![])
    }
    fn to_string(&self) -> String { format!("{:?}", self) }
}

impl OperationWriteExecute for MemberInitializationOperation {
    fn to_bytes(&self) -> BoxedErrorResult<Vec<u8>> {
        Ok(create_buf(&self, str_to_vec("MLIS")))
    }
    fn execute(&self, _source: Source) -> BoxedErrorResult<Vec<SendableOperation>> {
        merge_membership_list(&self.membership_list)?;
        merge_tcp_map(&self.udp_to_tcp_map)?;
        merge_all_file_owners(&self.all_file_owners)?;
        recalculate_neighbors()?;
        Ok(vec![])
    }
    fn to_string(&self) -> String { format!("{:?}", self) }
}
