use mongodb::{bson, doc, Bson};
use mongodb::db::ThreadedDatabase;
use mongodb::coll::Collection;
use mongodb::{Client, ThreadedClient};

use std::collections::{HashMap, HashSet};

pub struct PersistedMerkleTree {
    height: u32,
    bits_per_record: u32,
    collection: Collection,
    zero_hashes: Vec<String>,
}

fn bit_chunks(index: u32, bits_per_record: u32, height: u32) -> Vec<u32> {
    let mut chunks: Vec<u32> = (0..height)
        .step_by(bits_per_record as usize)
        .map(|i| (index >> i) % 2_u32.pow(bits_per_record))
        .collect();
    chunks.reverse();
    chunks
}

fn node_keys(chunks: &Vec<u32>) -> Vec<String> {
    let mut key = "".to_owned();
    let mut result = vec![];
    for chunk in chunks {
        result.push(key.clone());
        key = format!("{},{}", key, chunk);
    }
    result
}

type RecordMap = HashMap<String, Vec<String>>;

impl PersistedMerkleTree {
    pub fn new(height: u32, bits_per_record: u32) -> PersistedMerkleTree {
        let mut zero_hashes = vec!["value_0".to_owned()];
        for level in 0..height {
            let previous = zero_hashes.last().unwrap();
            let hash = format!("hash_0_l{}", level);
            zero_hashes.push(hash);
        }
        zero_hashes.reverse();
        let db = Client::connect("localhost", 27017)
            .unwrap()
            .db("BENCH");
        db.drop_collection("PersistedMerkleTree").unwrap();

        return PersistedMerkleTree {
            height,
            bits_per_record,
            collection: db.collection("PersistedMerkleTree"),
            zero_hashes,
        }
    }

    // Finds all nodes in the db with given key. Returns a map with all existing nodes' keys and their value array
    fn query_nodes<I>(&self, nodes: I) -> RecordMap 
        where I: IntoIterator<Item = String>
    {
        self.collection.find(Some(
            doc!{"key" => (
                doc!{"$in" => nodes.into_iter().map(|node| bson!{node}).collect::<Vec<Bson>>()}
            )}), None)
            .unwrap()
            .map(|result| result.unwrap())
            .map(|doc| (
                doc.get_str("key").unwrap().to_owned(), 
                doc.get_array("value").unwrap().iter().map(|bson| bson.as_str().unwrap().to_owned()).collect()
            )).collect::<HashMap<String, Vec<String>>>()
    }

    fn write_nodes(&self, pages: RecordMap, state_index: u32) -> () {
        let documents = pages.iter().map(|(key, page)| doc!{
            "key" => key, 
            "value" => page.iter().map(|i| Bson::from(i)).collect::<Vec<Bson>>(), 
            "state_index" => state_index 
        }).collect();
        self.collection.insert_many(documents, None).unwrap();
    }

    pub fn query_record_and_pages(&self, chunks: &Vec<u32>, existing_nodes: RecordMap) -> (String, Vec<String>, RecordMap) {
        let nodes = node_keys(&chunks);
        assert_eq!(&chunks.len(), &nodes.len());
        let mut existing_nodes = existing_nodes;

        // Fetch merkle path
        let mut merkle_path = vec![];
        let mut pages = HashMap::new();
        for chunk_index in 0..chunks.len() {
            let chunk = &chunks[chunk_index];
            let node_key = &nodes[chunk_index];
            
            let page = existing_nodes.remove(node_key).unwrap_or_else(|| {
                // Create empty page
                let mut page = vec![];
                for level_within_chunk in 0..self.bits_per_record {
                    let level = chunk_index as u32 * self.bits_per_record + (self.bits_per_record - level_within_chunk);
                    page.extend(vec![self.zero_hashes[level as usize].clone(); 2_usize.pow(self.bits_per_record - level_within_chunk)])
                }
                return page;
            });

            let mut offset_in_value_array = 0;
            let mut reverse_sub_path = vec![];
            for level_within_chunk in 0..self.bits_per_record {
                let index_in_value_array = offset_in_value_array + (chunk >> level_within_chunk) as usize;
                let sibling = if index_in_value_array % 2 == 1 {
                    page[index_in_value_array - 1].clone()
                } else {
                    page[index_in_value_array + 1].clone()
                };
                reverse_sub_path.push(sibling);
                offset_in_value_array += 2_usize.pow(self.bits_per_record - level_within_chunk);
            }
            reverse_sub_path.reverse();
            merkle_path.extend(reverse_sub_path);
            pages.insert(node_key.clone(), page);
        }
        
        // Fetch node value
        let last_node_key = nodes.last().unwrap();
        let node = pages[last_node_key][chunks.last().unwrap().to_owned() as usize].clone();
        (node, merkle_path, pages)
    }

    pub fn query_record(&self, index: u32) -> (String, Vec<String>) {
        let chunks = bit_chunks(index, self.bits_per_record, self.height);
        let (item, proof, _) = self.query_record_and_pages(&chunks, self.query_nodes(node_keys(&chunks)));
        (item, proof)
    }

    pub fn update_records<F>(&self, indices: Vec<u32>, update_fn: F, state_index: u32) -> String 
        where F: Fn(u32, String) -> String
    {
        let list_of_chunks = indices.iter().map(
            |index| bit_chunks(*index, self.bits_per_record, self.height)
        ).collect::<Vec<Vec<u32>>>();

        let node_keys = list_of_chunks.iter().flat_map(node_keys).collect::<HashSet<String>>();
        let mut records = self.query_nodes(node_keys.clone());
        let mut root = "".to_owned();
        for chunks in list_of_chunks {
            let (item, proof, pages) = self.query_record_and_pages(&chunks, records.clone());
            let tuple = self.update_pages_with_item(update_fn(0, item), proof, pages, &chunks);
            root = tuple.0;
            records = tuple.1;
        }
        self.write_nodes(records, state_index);
        return root;
    }

    #[allow(dead_code)]
    pub fn update_record<F>(&self, index: u32, update_fn: F, state_index: u32) -> String 
        where F: FnOnce(String) -> String
    {
        let chunks = bit_chunks(index, self.bits_per_record, self.height);
        let nodes = node_keys(&chunks);

        let (item, proof, pages) = self.query_record_and_pages(&chunks, self.query_nodes(nodes));
        let (root, pages) = self.update_pages_with_item(update_fn(item), proof, pages, &chunks);
        self.write_nodes(pages, state_index);

        return root;
    }

    fn update_pages_with_item(&self, item: String, proof: Vec<String>, pages: RecordMap, chunks: &Vec<u32>) -> (String, RecordMap) {
        let nodes = node_keys(chunks);
        let mut pages = pages;
        let mut proof = proof;
        let mut item = item;
        for reverse_index in 0..self.height {
            let level = self.height - (reverse_index + 1);
            let reverse_level_within_chunk = reverse_index % self.bits_per_record;

            let chunk_index = (level/self.bits_per_record) as usize;
            let page = pages.get_mut(&nodes[chunk_index]).unwrap();
            let chunk = chunks[chunk_index];

            let page_offset: usize = (0..reverse_level_within_chunk).map(|i| 2_usize.pow(self.bits_per_record - i)).sum();
            let page_index = page_offset + (chunk >> reverse_level_within_chunk) as usize;
            page[page_index] = item.clone();

            item = format!("h({},{})", proof.pop().unwrap(), item)
        }
        (item, pages)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::{Mutex};

    #[test]
    fn test_bit_chunks() {
        assert_eq!(bit_chunks(10, 1, 4), vec![1,0,1,0]);
        assert_eq!(bit_chunks(10, 2, 4), vec![2,2]);
        assert_eq!(bit_chunks(10, 3, 4), vec![1,2]);
        assert_eq!(bit_chunks(10, 4, 4), vec![10]);
        assert_eq!(bit_chunks(10, 5, 4), vec![10]);

        assert_eq!(bit_chunks(10, 1, 5), vec![0, 1,0,1,0]);
        assert_eq!(bit_chunks(10, 4, 5), vec![0, 10]);
        assert_eq!(bit_chunks(10, 5, 5), vec![10]);
    }

    #[test]
    fn test_node_keys() {
        assert_eq!(node_keys(&vec![10,1,432]), vec!["", ",10", ",10,1"])
    }

    #[test]

    fn test_first_fetch() {
        let tree = PersistedMerkleTree::new(24, 3);
        let (node, path) = tree.query_record(0);
        assert_eq!(path.len(), 24);
        assert_eq!(node, "value_0");
    }

    #[test]
    fn test_first_update() {
        let tree = PersistedMerkleTree::new(24, 3);
        let root = tree.update_record(0, |_| "1".to_owned(), 1);
        assert_eq!(root, "h(hash_0_l22,h(hash_0_l21,h(hash_0_l20,h(hash_0_l19,h(hash_0_l18,h(hash_0_l17,h(hash_0_l16,h(hash_0_l15,h(hash_0_l14,h(hash_0_l13,h(hash_0_l12,h(hash_0_l11,h(hash_0_l10,h(hash_0_l9,h(hash_0_l8,h(hash_0_l7,h(hash_0_l6,h(hash_0_l5,h(hash_0_l4,h(hash_0_l3,h(hash_0_l2,h(hash_0_l1,h(hash_0_l0,h(value_0,1))))))))))))))))))))))))");

        let (node, path) = tree.query_record(0);
        assert_eq!(path.len(), 24);
        assert_eq!(node, "1");
    }

    #[test]
    fn test_batch_update() {
        let updates = Mutex::new(vec![
            (0, "u1".to_owned()),
            (1, "u2".to_owned()),
            (1, "u3".to_owned()),
            (1000000, "u4".to_owned()),
            (2u32.pow(24)-1, "u5".to_owned())
        ]);

        let tree = PersistedMerkleTree::new(24, 3);
        let mut root_after_individual_updates = "".to_owned();
        for (i, (address, value)) in updates.lock().unwrap().iter().enumerate() {
            root_after_individual_updates = tree.update_record(*address, |_|value.clone(), i as u32);
        }

        let tree = PersistedMerkleTree::new(24, 3);
        let addresses : Vec<u32> = updates.lock().unwrap().iter().map(|(address, _)|*address).collect();
        let root_after_batch_updates = tree.update_records(addresses, |_, _| updates.lock().unwrap().remove(0).1, 1);

        assert_eq!(root_after_individual_updates, root_after_batch_updates);
    }
 
}