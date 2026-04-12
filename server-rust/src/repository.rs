use mongodb::{
    bson::{doc, Bson, DateTime, Document},
    options::ClientOptions,
    Client, Collection, IndexModel,
};
use futures::TryStreamExt;
use serde_json::{json, Value};
use tracing::{error, info};

#[derive(Clone)]
pub struct ActionRepository {
    collection: Option<Collection<Document>>,
}

impl ActionRepository {
    pub async fn connect(uri: &str) -> Self {
        let options = match ClientOptions::parse(uri).await {
            Ok(v) => v,
            Err(err) => {
                error!("mongodb options error: {}", err);
                return Self { collection: None };
            }
        };

        let db_name = options
            .default_database
            .clone()
            .unwrap_or_else(|| "virus_game".to_string());

        let client = match Client::with_options(options) {
            Ok(c) => c,
            Err(err) => {
                error!("mongodb client error: {}", err);
                return Self { collection: None };
            }
        };

        let db = client.database(&db_name);
        if let Err(err) = db.run_command(doc! {"ping": 1}).await {
            error!("mongodb ping error: {}", err);
            return Self { collection: None };
        }

        let collection_name = "actions";
        let collection_exists = match db.list_collection_names().await {
            Ok(names) => names.iter().any(|name| name == collection_name),
            Err(err) => {
                error!("failed listing collections: {}", err);
                false
            }
        };

        if !collection_exists {
            match db.create_collection(collection_name).await {
                Ok(_) => info!("created collection {}.{}", db_name, collection_name),
                Err(err) => {
                    error!("failed creating collection {}: {}", collection_name, err);
                    return Self { collection: None };
                }
            }
        }

        let collection = db.collection::<Document>(collection_name);

        // Índices para consultas de historial por sesión y orden temporal.
        let session_time_index = IndexModel::builder()
            .keys(doc! {"sessionId": 1, "createdAt": 1})
            .build();
        let action_type_index = IndexModel::builder()
            .keys(doc! {"sessionId": 1, "actionType": 1, "createdAt": 1})
            .build();

        if let Err(err) = collection.create_index(session_time_index).await {
            error!("failed creating session/time index: {}", err);
        }
        if let Err(err) = collection.create_index(action_type_index).await {
            error!("failed creating actionType index: {}", err);
        }

        info!("connected to mongodb using database {}", db_name);

        Self {
            collection: Some(collection),
        }
    }

    pub async fn log_action(
        &self,
        session_id: &str,
        action_type: &str,
        actor_id: Option<&str>,
        payload: Value,
    ) {
        let Some(collection) = &self.collection else {
            return;
        };

        let mut doc = doc! {
            "sessionId": session_id,
            "actionType": action_type,
            "createdAt": DateTime::now(),
            "payload": mongodb::bson::to_bson(&payload).unwrap_or_default(),
        };

        if let Some(actor) = actor_id {
            doc.insert("actorId", actor);
        }

        if let Err(err) = collection.insert_one(doc).await {
            error!("failed to insert action log: {}", err);
        }
    }

    pub async fn fetch_actions(
        &self,
        session_id: &str,
        action_type: Option<&str>,
        limit: u64,
        offset: u64,
    ) -> (u64, Vec<Value>) {
        let Some(collection) = &self.collection else {
            return (0, Vec::new());
        };

        let mut filter = doc! {"sessionId": session_id};
        if let Some(a) = action_type {
            filter.insert("actionType", a);
        }

        let total = match collection.count_documents(filter.clone()).await {
            Ok(v) => v,
            Err(err) => {
                error!("failed to count actions: {}", err);
                0
            }
        };

        let cursor = match collection
            .find(filter)
            .sort(doc! {"createdAt": 1})
            .skip(offset)
            .limit(limit as i64)
            .await
        {
            Ok(c) => c,
            Err(err) => {
                error!("failed to query actions: {}", err);
                return (total, Vec::new());
            }
        };

        let mut cursor = cursor;
        let mut items = Vec::new();
        while let Ok(next) = cursor.try_next().await {
            let Some(doc) = next else {
                break;
            };

            match mongodb::bson::from_bson::<Value>(Bson::Document(doc)) {
                Ok(value) => items.push(value),
                Err(err) => error!("failed to decode bson document to json: {}", err),
            }
        }

        (total, items)
    }

    pub async fn get_status(&self) -> Value {
        let Some(collection) = &self.collection else {
            return json!({
                "ok": false,
                "message": "repository sin conexion a MongoDB"
            });
        };

        let namespace = collection.namespace();
        let mut index_count: u64 = 0;

        let indexes_result = collection.list_indexes().await;
        if let Ok(mut cursor) = indexes_result {
            while let Ok(next) = cursor.try_next().await {
                if next.is_none() {
                    break;
                }
                index_count += 1;
            }
        }

        let action_count = collection
            .estimated_document_count()
            .await
            .unwrap_or(0);

        json!({
            "ok": true,
            "database": namespace.db,
            "collection": namespace.coll,
            "indexes_count": index_count,
            "estimated_actions": action_count
        })
    }
}
