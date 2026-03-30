use crate::domain::models::file_cache_models::CacheChannel;
use rkv::backend::{SafeMode, SafeModeDatabase, SafeModeEnvironment};
use rkv::{Manager, Rkv, SingleStore, StoreOptions, Value};
use std::error::Error;
use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock};

pub static RKV_SERVICE: RwLock<Option<RkvService>> = RwLock::new(None);

pub fn initialize_rkv(main_path: String) {
    let guard = RKV_SERVICE.write();
    if guard.is_err() {
        return;
    }
    let mut guard = guard.unwrap();
    if guard.is_some() {
        return;
    }
    let instance = RkvService::new(main_path);
    *guard = Some(instance);
}

pub struct RkvService {
    pub main_path: String,
    env: Option<Arc<RwLock<Rkv<SafeModeEnvironment>>>>,
}

impl RkvService {
    pub fn new(main_path: String) -> Self {
        Self {
            main_path,
            env: None,
        }
    }

    pub fn init_db(&mut self, name: &str) -> Result<SingleStore<SafeModeDatabase>, Box<dyn Error>> {
        if self.env.is_none() {
            let path = self.main_path.as_str();
            fs::create_dir_all(path)?;

            let mut manager = Manager::<SafeModeEnvironment>::singleton().write()?;
            let created_arc = manager
                .get_or_create(Path::new(path), Rkv::new::<SafeMode>)
                .unwrap();
            self.env = Some(created_arc);
        }

        let store = self
            .env
            .as_ref()
            .unwrap()
            .read()
            .unwrap()
            .open_single(name, StoreOptions::create())?;
        Ok(store)
    }

    pub fn write_rkyv_cache_channel_data(
        &self,
        store: &SingleStore<SafeModeDatabase>,
        key: &str,
        data: &CacheChannel,
    ) -> Result<(), Box<dyn Error>> {
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(data)
            .map_err(|e| format!("rkyv serialization failed: {}", e))?;

        let env = self.env.as_ref().unwrap().read().unwrap();
        let mut writer = env.write()?;
        store.put(&mut writer, key, &Value::Blob(&bytes))?;
        writer.commit()?;

        Ok(())
    }

    pub fn read_rkyv_cache_channel_data(
        &self,
        store: &SingleStore<SafeModeDatabase>,
        key: &str,
    ) -> Result<Option<CacheChannel>, Box<dyn Error>> {
        let env = self.env.as_ref().unwrap().read().unwrap();
        let reader = env.read()?;
        match store.get(&reader, key)? {
            None => Ok(None),
            Some(Value::Blob(bytes)) => {
                let archived =
                    rkyv::from_bytes::<CacheChannel, bytecheck::rancor::Error>(&bytes.to_vec())?;
                Ok(Some(archived))
            }
            Some(_) => Err("unknown type".into()),
        }
    }
}
