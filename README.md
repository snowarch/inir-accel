# `inir-rust-plugin` — Native Rust Runtime, Vision & Theme Accelerator + QML Plugin for iNiR

Suite de aceleración nativa en Rust para la shell **[iNiR](file:///home/user/inir)** diseñada para convivir sin solapamientos con **[`inir-cli`](file:///home/user/inir-cli)** (CLI/IPC/doctor/deps en Go) y **[`qs-rs`](file:///home/user/inir-qs-rust)** (host QML en Rust), sin modificar ni un solo byte de `/home/user/inir`.

## 1. Qué hace (y por qué no choca con `inir-cli` ni `qs-rs`)

- **`inir-cli` (`/home/user/inir-cli`)** ya resuelve el plano externo de comandos (`inir <ipc>`, `inir doctor`, `inir deps`, `inir completions`).
- **`qs-rs` (`/home/user/inir-qs-rust`)** ya resuelve el host de Quickshell.
- **`inir-rust-plugin` (`/home/user/inir-rust-plugin`)** acelera el **plano interno en vivo de la sesión gráfica**: los procesos pesados de visión por computadora (OpenCV/NumPy), extracción cromática Material You + HCT + APCA (PIL/NumPy/MaterialYouColor), derivación perceptual Washi v30, generación masiva de miniaturas (`thumbgen.py`) y telemetría de hardware que la shell dispara en caliente mientras la usás.

---

## 2. Arquitectura del Workspace

1. **[`crates/inir-core`](file:///home/user/inir-rust-plugin/crates/inir-core/src/lib.rs)**:
   - [`region.rs`](file:///home/user/inir-rust-plugin/crates/inir-core/src/region.rs): Reemplazo nativo en Rust de `scripts/images/least_busy_region.py` (sin OpenCV ni NumPy). Implementa escalado de pantalla (`fill`, `fit`, `stretch`, `center`, `tile`), caché binario `.rbin` en `~/.cache/inir/region-sampling/`, grillas `--luma-grid 24` para `services/WallpaperLuma.qml`, imágenes integrales 2D $O(1)$ para búsqueda de regiones (`--busiest`, `--largest-region`) y K-means determinista ($K=3$) para color dominante de widgets.
   - [`material.rs`](file:///home/user/inir-rust-plugin/crates/inir-core/src/material.rs): Reemplazo nativo de `scripts/colors/generate_colors_material.py`. Extracción de semilla cromática desde imagen, autodetección de esquema Hasler-Süsstrunk (`--scheme auto`), solver de contraste WCAG 2.1 + APCA-W3 `Lc`, armonización de colores de terminal, anclaje `iris-surface.json` (`--washi-roles`) y motor de plantillas Matugen (`templates.json`).
   - [`washi.rs`](file:///home/user/inir-rust-plugin/crates/inir-core/src/washi.rs): Puerto completo en Rust del motor perceptual Washi v30 (`washi`, `iris`, `airisu` y `material:*`).
   - [`thumbgen.rs`](file:///home/user/inir-rust-plugin/crates/inir-core/src/thumbgen.rs): Reemplazo paralelo con Rayon de `scripts/thumbnails/thumbgen.py` con hash MD5 RFC 1321 idéntico a `ThumbnailImage.qml` (`~/.cache/thumbnails/<size>/<md5>.png`) y streaming `PROGRESS i/total FILE ...` para `services/Wallpapers.qml`.
   - [`sensors.rs`](file:///home/user/inir-rust-plugin/crates/inir-core/src/sensors.rs): Descubrimiento `hwmon`/GPU (`detect_sensors.py`) y lectura directa de `/proc/meminfo`, `/proc/stat`, `/proc/cpuinfo` y `statvfs`.
   - [`apps.rs`](file:///home/user/inir-rust-plugin/crates/inir-core/src/apps.rs): Escaneo ultrarrápido de `.desktop`, búsqueda difusa + Levenshtein (`AppSearch.qml`), resolución de íconos de ventana, `scan-plugins.py`, `scan-widgets.sh` e `icon-theme-for-mode.sh`.
   - [`prewarm.rs`](file:///home/user/inir-rust-plugin/crates/inir-core/src/prewarm.rs): Escáner y constructor paralelo de VFS QML (1514 archivos QML) compatible con `qs-rs`.
2. **[`crates/inir-accel`](file:///home/user/inir-rust-plugin/crates/inir-accel/src/main.rs)**:
   - Binario multi-call (`inir-accel`) que funciona tanto por subcomandos directos como por intercepción transparente de `~/.local/state/quickshell/.venv/bin/python3` y `~/.local/state/quickshell/user/generated/bin/inir-washi`.
   - Cuando la shell en ejecución invoca `~/.local/state/quickshell/.venv/bin/python3` sobre `generate_colors_material.py`, `least_busy_region.py`, `thumbgen.py`, `detect_sensors.py` o `scan-plugins.py`, `inir-accel` los ejecuta nativamente en Rust; para cualquier otro script de Python (`ytmusic`, `innertube`, `-c`, `-m`), delega instantáneamente vía `execv` a `python3.real`.
3. **[`crates/inir-qml-plugin`](file:///home/user/inir-rust-plugin/crates/inir-qml-plugin/src/lib.rs)**:
   - Plugin QML nativo Qt6 (`import Inir.Native 1.0`, singleton `InirNative`) construido con `cxx-qt 0.10.0` que expone propiedades reactivas de telemetría y métodos `#[qinvokable]` sin crear subprocesos (`refreshResources`, `solveWashi`, `generateMaterialColors`, `computeLumaGridJson`, `makeThumbnail`, `fuzzySearchAppsJson`, `guessIcon`, `scanWidgetsJson`, `scanPluginsJson`, `prewarmVfsJson`).

---

## 3. Benchmarks Reales en Vivo (Medidos sobre la sesión actual)

Pruebas A/B ejecutadas sobre el wallpaper activo de alta resolución (`ChatGPT Image ..._waifu2x_art_noise3_scale.png`) y la carpeta `/home/user/Imágenes/Wallpapers` (350 archivos):

| Operación en Vivo de iNiR | Stack Original (Python/OpenCV/PIL) | `inir-rust-plugin` (Rust en vivo) | Mejora |
| :--- | ---: | ---: | ---: |
| `WallpaperLuma` (`least_busy_region.py --luma-grid 24`, frío) | `501 ms` | **`126 ms`** | **4.0× más rápido** |
| `WallpaperLuma` / Widgets (`least_busy_region.py`, caché `.rbin`) | `501 ms` | **`14 ms`** | **35.8× más rápido** |
| `generate_colors_material.py` (tiempo real / tiempo de CPU) | `295 ms` (`1270 ms` CPU) | **`48 ms`** (`46 ms` CPU) | **6.1× más rápido (27.6× menos CPU)** |
| `thumbgen-venv.sh` (350 wallpapers en `/home/user/Imágenes/Wallpapers`) | ~3.5s – 12s | **`459 ms`** | **~8×–25× más rápido** |
| `inir-washi` (3 esquemas + 7 estilos) | ~18–25 ms | **`12.7 ms`** (binario) / **`0.42 ms`** (Material in-proc) | Instantáneo |
| Telemetría hardware (`detect_sensors` + `/proc` + `statvfs`) | ~45 ms | **`0.78 ms`** | **57× más rápido** |

---

## 4. Uso y Activación en Vivo (Zero-Touch)

El script [`scripts/activate-accelerators.sh`](file:///home/user/inir-rust-plugin/scripts/activate-accelerators.sh) activa o desactiva los hooks en `~/.local/state/quickshell/` sin tocar ningún archivo de `/home/user/inir`:

```bash
# Activar aceleradores en vivo (ya activos en tu sesión actual):
/home/user/inir-rust-plugin/scripts/activate-accelerators.sh install

# Verificar estado de los hooks activos:
/home/user/inir-rust-plugin/scripts/activate-accelerators.sh status

# Correr suite de benchmarks:
/home/user/inir-rust-plugin/bin/inir-accel bench

# Desactivar y restaurar exactamente el python3 del venv y el inir-washi original:
/home/user/inir-rust-plugin/scripts/activate-accelerators.sh uninstall
```

---

## 5. Color: neutros cálidos, acentos del wallpaper

`inir-accel` le da a los neutros (superficies, subtextos, bordes, `term7`) un matiz arena/piedra cálido controlado por el slider **Warmth** (`tune.<scheme>.warmth`), mientras `primary`, `secondary`, `tertiary`, accent y highlight siguen siempre al wallpaper y a los ajustes de Material/iRiS. Detalle, roles, bug corregido y verificación en [`docs/COLOR_WARMTH.md`](docs/COLOR_WARMTH.md).

---
This code was written by: Hikari.
