use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha1::{Digest, Sha1};
use std::io::Write;

pub struct RepoBuilder {
    objects: Vec<Vec<u8>>,
}

impl RepoBuilder {
    pub fn new() -> Self {
        Self {
            objects: Vec::new(),
        }
    }

    fn hash_object(obj_type: &str, content: &[u8]) -> (String, Vec<u8>, Vec<u8>) {
        let header = format!("{} {}\0", obj_type, content.len());
        let mut data = header.into_bytes();
        data.extend_from_slice(content);

        let mut hasher = Sha1::new();
        hasher.update(&data);
        let hash = hex::encode(hasher.finalize());
        (hash, data, content.to_vec())
    }

    fn pack_object_header(obj_type: u8, mut size: usize) -> Vec<u8> {
        let mut header = Vec::new();
        let mut c = (obj_type << 4) | ((size as u8) & 0x0f);
        size >>= 4;
        if size > 0 {
            c |= 0x80;
        }
        header.push(c);
        while size > 0 {
            let mut c = (size as u8) & 0x7f;
            size >>= 7;
            if size > 0 {
                c |= 0x80;
            }
            header.push(c);
        }
        header
    }

    fn add_object(&mut self, obj_type: u8, content: &[u8]) {
        let mut packed_obj = Self::pack_object_header(obj_type, content.len());
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(content).unwrap();
        let compressed = encoder.finish().unwrap();
        packed_obj.extend(compressed);
        self.objects.push(packed_obj);
    }

    pub fn create_blob(&mut self, content: &[u8]) -> String {
        let (hash, _, content) = Self::hash_object("blob", content);
        self.add_object(3, &content);
        hash
    }

    pub fn create_tree(&mut self, filename: &str, blob_hash: &str) -> String {
        let mut content = Vec::new();
        content.extend_from_slice(b"100644 ");
        content.extend_from_slice(filename.as_bytes());
        content.push(0);
        let raw_hash = hex::decode(blob_hash).unwrap();
        content.extend_from_slice(&raw_hash);

        let (hash, _, content) = Self::hash_object("tree", &content);
        self.add_object(2, &content);
        hash
    }

    pub fn create_commit(
        &mut self,
        tree_hash: &str,
        parent_hash: Option<&str>,
        author: &str,
        timestamp: u64,
        message: &str,
    ) -> String {
        let mut content = String::new();
        content.push_str(&format!("tree {}\n", tree_hash));
        if let Some(p) = parent_hash {
            content.push_str(&format!("parent {}\n", p));
        }
        // timezone +0000 for simplicity
        content.push_str(&format!("author {} {} +0000\n", author, timestamp));
        content.push_str(&format!("committer {} {} +0000\n\n", author, timestamp));
        content.push_str(message);
        content.push('\n');

        let (hash, _, content) = Self::hash_object("commit", content.as_bytes());
        self.add_object(1, &content);
        hash
    }

    pub fn build_pack(&self) -> Vec<u8> {
        let mut pack = Vec::new();
        pack.extend_from_slice(b"PACK");
        pack.extend_from_slice(&2u32.to_be_bytes()); // Version 2
        pack.extend_from_slice(&(self.objects.len() as u32).to_be_bytes()); // Object count

        for obj in &self.objects {
            pack.extend(obj);
        }

        let mut hasher = Sha1::new();
        hasher.update(&pack);
        let checksum = hasher.finalize();
        pack.extend_from_slice(&checksum);
        pack
    }
}
