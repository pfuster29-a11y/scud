<!-- Documentación redactada y estructurada con la asistencia de un LLM -->

# Scud 🚀

> *Nota: Tanto la documentación como el código fuente de este proyecto fueron desarrollados con la asistencia de un LLM.*

Herramienta en **Rust** diseñada para automatizar, simplificar y blindar la migración y actualización de sistemas Debian hacia **Debian Sid (Unstable)**, evitando falsos positivos y errores comunes de repositorios.

## ✨ Características Principales

* **Migración Inteligente de Repositorios (`sources.list`):**
  * Genera un respaldo automático (`.bak`) antes de realizar cualquier cambio.
  * Comenta automáticamente los repositorios de seguridad y actualizaciones (`-updates`) incompatibles con Sid en lugar de borrarlos, evitando errores HTTP 404 y preservando tus preferencias.
  * Deduplica entradas de URLs mediante un análisis preciso por tokens.
* **Simulación Segura:** Previsualiza los paquetes a actualizar mediante `apt-get dist-upgrade --just-print` sin alterar tu sistema operativo.
* **Streaming de Terminal en Vivo:** Utiliza `.status()` junto a `pkexec` para transmitir la salida de APT directamente a la terminal, evitando falsas alarmas por advertencias no fatales (`stderr`) durante actualizaciones masivas (más de 1,400 paquetes).

---

## 🛠️ Compilación e Instalación

Asegurate de tener instalado **Rust** y **Cargo** en tu sistema. Luego, cloná el repositorio y compilá en modo release:

```bash
git clone [https://github.com/pfuster29-a11y/scud.git](https://github.com/pfuster29-a11y/scud.git)
cd scud
cargo build --release
