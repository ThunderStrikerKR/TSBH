
        
        PRAGMA busy_timeout=5000;
        PRAGMA foreign_keys=ON;
        CREATE TABLE IF NOT EXISTS app_state(
            id INTEGER PRIMARY KEY CHECK(id=1),payload TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS product_index(
            id TEXT PRIMARY KEY,code TEXT NOT NULL,name TEXT NOT NULL,payload TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_product_code ON product_index(code);
        CREATE INDEX IF NOT EXISTS idx_product_name ON product_index(name);
        CREATE TABLE IF NOT EXISTS order_index(
            id TEXT PRIMARY KEY,client_id TEXT NOT NULL,created_at TEXT NOT NULL,
            confirmed_at TEXT,status TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_order_created ON order_index(created_at);
        CREATE INDEX IF NOT EXISTS idx_order_client ON order_index(client_id);
        CREATE TABLE IF NOT EXISTS receipt_archive(
            id TEXT PRIMARY KEY,order_id TEXT NOT NULL,record TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_receipt_order ON receipt_archive(order_id);
    
        CREATE TABLE IF NOT EXISTS core_schema(
            key TEXT PRIMARY KEY, version INTEGER NOT NULL CHECK(version > 0)
        );
        CREATE TABLE IF NOT EXISTS core_products(
            id TEXT PRIMARY KEY, position INTEGER NOT NULL,
            code TEXT NOT NULL, name TEXT NOT NULL, payload TEXT NOT NULL CHECK(json_valid(payload))
        );
        CREATE INDEX IF NOT EXISTS idx_core_products_name ON core_products(name);
        CREATE INDEX IF NOT EXISTS idx_core_products_code ON core_products(code);
        CREATE TABLE IF NOT EXISTS core_product_prices(
            product_id TEXT PRIMARY KEY REFERENCES core_products(id) ON DELETE CASCADE,
            price_record_id TEXT NOT NULL UNIQUE,
            color TEXT NOT NULL,
            wholesale INTEGER CHECK(wholesale IS NULL OR wholesale >= 0),
            semi INTEGER CHECK(semi IS NULL OR semi >= 0),
            retail INTEGER CHECK(retail IS NULL OR retail >= 0),
            payload TEXT NOT NULL CHECK(json_valid(payload))
        );
        CREATE TABLE IF NOT EXISTS core_clients(
            id TEXT PRIMARY KEY, position INTEGER NOT NULL, name TEXT NOT NULL,
            tier TEXT NOT NULL, address TEXT NOT NULL, payload TEXT NOT NULL CHECK(json_valid(payload))
        );
        CREATE INDEX IF NOT EXISTS idx_core_clients_name ON core_clients(name);
        CREATE TABLE IF NOT EXISTS core_orders(
            id TEXT PRIMARY KEY, position INTEGER NOT NULL, client_id TEXT,
            created_at TEXT, confirmed_at TEXT, status TEXT,
            payload TEXT NOT NULL CHECK(json_valid(payload))
        );
        CREATE INDEX IF NOT EXISTS idx_core_orders_created ON core_orders(created_at);
        CREATE INDEX IF NOT EXISTS idx_core_orders_client ON core_orders(client_id);
        CREATE TABLE IF NOT EXISTS core_recipients(
            id TEXT PRIMARY KEY, order_id TEXT NOT NULL REFERENCES core_orders(id) ON DELETE CASCADE,
            position INTEGER NOT NULL, name TEXT NOT NULL, payload TEXT NOT NULL CHECK(json_valid(payload))
        );
        CREATE INDEX IF NOT EXISTS idx_core_recipients_order ON core_recipients(order_id,position);
        CREATE TABLE IF NOT EXISTS core_items(
            id TEXT PRIMARY KEY, recipient_id TEXT NOT NULL REFERENCES core_recipients(id) ON DELETE CASCADE,
            position INTEGER NOT NULL, product_id TEXT, variant_id TEXT,
            payload TEXT NOT NULL CHECK(json_valid(payload))
        );
        CREATE INDEX IF NOT EXISTS idx_core_items_recipient ON core_items(recipient_id,position);
        CREATE INDEX IF NOT EXISTS idx_core_items_product ON core_items(product_id);
    CREATE TABLE IF NOT EXISTS reception_working_orders (
            id TEXT PRIMARY KEY,
            changed_at TEXT NOT NULL,
            payload TEXT NOT NULL CHECK(json_valid(payload))
        );
        CREATE TABLE IF NOT EXISTS reception_confirmed_edits (
            id TEXT PRIMARY KEY,
            changed_at TEXT NOT NULL,
            base_payload TEXT NOT NULL CHECK(json_valid(base_payload)),
            payload TEXT NOT NULL CHECK(json_valid(payload))
        );CREATE TABLE IF NOT EXISTS hub_notifications(
            id TEXT PRIMARY KEY NOT NULL,
            at TEXT NOT NULL,
            read INTEGER NOT NULL DEFAULT 0 CHECK(read IN (0, 1)),
            payload TEXT NOT NULL
        );
INSERT INTO core_schema(key,version) VALUES('business-core',2);
