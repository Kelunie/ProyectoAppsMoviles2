# Virus Game Server (Rust + MongoDB)

Backend completo del juego. La logica se ejecuta en servidor.

## Requisitos

1. Rust (cargo)
2. MongoDB Server corriendo como servicio local

## Variables de entorno

- `PORT` (opcional, default `3000`)
- `MONGODB_URI` (opcional, default `mongodb://127.0.0.1:27017/virus_game`)

Ejemplo (PowerShell):

```powershell
$env:PORT="3000"
$env:MONGODB_URI="mongodb://127.0.0.1:27017/virus_game"
```

## Ejecutar

```powershell
cd server-rust
cargo run
```

Servidor:

- HTTP: `http://localhost:3000`
- WebSocket: `ws://localhost:3000/ws`

## Archivos versionados vs generados

Archivos/carpetas que SI se suben a git:

- Codigo fuente: `server-rust/src/**`
- Configuracion de Rust: `server-rust/Cargo.toml`, `server-rust/Cargo.lock`
- Configuracion de build policy workaround: `server-rust/.cargo/config.toml`
- Documentacion: `README.md`

Archivos/carpetas que NO se suben (se generan localmente):

- `server-rust/target/` (artefactos de compilacion)
- archivos `.env` locales

Comandos para regenerar lo necesario:

```powershell
cd server-rust

# compilar (genera target/)
cargo build

# ejecutar servidor
cargo run
```

Si necesitas recrear lockfile:

```powershell
cd server-rust
cargo generate-lockfile
```

## Flujo recomendado para frontend

1. Crear sala por HTTP.
2. Listar salas abiertas por HTTP.
3. Conectar WebSocket y hacer `join` con `room_id`, `user_id`, `name`.
4. Consumir `public_state` para actualizar UI en tiempo real.
5. Usar endpoints de historial para espectador/replay.

## Contrato de identidad

El usuario lo crea el telefono. El frontend debe enviar:

- `user_id`: id unico de la cuenta
- `name`: nombre visible

Evento `join` obligatorio:

```json
{
  "type": "join",
  "room_id": "ROOM_UUID",
  "user_id": "user-123",
  "name": "Julio"
}
```

## Endpoints HTTP

### Salud y catalogo

1. `GET /health`
2. `GET /api/endpoints`

### Salas

3. `POST /rooms`

Body:

```json
{
  "name": "Sala 1",
  "host_user_id": "host-1"
}
```

4. `GET /rooms/open`
5. `POST /rooms/:room_id/close`

Body:

```json
{
  "requester_user_id": "host-1"
}
```

6. `POST /rooms/:room_id/reopen`

Body:

```json
{
  "requester_user_id": "host-1"
}
```

7. `GET /rooms/:room_id/state`
8. `GET /rooms/:room_id/players/:player_id/role`

### Historial (Mongo)

9. `GET /rooms/:room_id/actions?limit=100&offset=0&action_type=vote`
10. `GET /rooms/:room_id/chat?limit=100&offset=0`

## Eventos WebSocket

### Cliente -> servidor

1. Join a sala

```json
{ "type": "join", "room_id": "ROOM_UUID", "user_id": "user-1", "name": "Ana" }
```

2. Iniciar juego

```json
{ "type": "start_game" }
```

3. Infectar

```json
{ "type": "terror_infect", "target_id": "user-2" }
```

4. Investigar

```json
{ "type": "investigate", "target_id": "user-3" }
```

5. Votar

```json
{ "type": "vote", "target_id": "user-4" }
```

6. Chat

```json
{ "type": "send_chat", "message": "hola" }
```

7. Avanzar fase

```json
{ "type": "advance_phase" }
```

### Servidor -> cliente

- `joined`
- `public_state`
- `info`
- `error`

Nota: el servidor emite eventos de tiempo real filtrados por sala, cada socket recibe solo eventos de su `room_id` unido.

## Reglas activas

- Minimo 8 jugadores, maximo 10.
- Roles: 2 terroristas, 1 investigador, 1 fanatico, resto ciudadanos.
- Fases: `secret_actions -> discussion -> voting -> resolution`.
- Votacion: maximo 120 segundos.
- Chat: 1 mensaje cada 6 segundos por jugador.
- Infeccion: mata en 2 rondas.
- Cura: se desbloquea al progreso 3 del investigador.

## Persistencia MongoDB

Coleccion: `actions`

Se registran eventos como:

- `room_created`
- `join`
- `join_reconnect`
- `room_closed`
- `room_reopened`
- `start_game`
- `terror_infect`
- `investigate`
- `vote`
- `chat_message`
- `voting_result`
- `infection_resolution`
- `advance_phase`
- `game_end`

Cada registro guarda metadatos de sesion y `room_id` en payload para reconstruccion por sala.

## Nota Windows (App Control 4551)

Si aparece `An Application Control policy has blocked this file (os error 4551)`, este repo ya usa:

- `server-rust/.cargo/config.toml`
- `target-dir = "C:/RustBuilds/virus-game-server"`

para evitar ejecutar binarios dentro de OneDrive.
