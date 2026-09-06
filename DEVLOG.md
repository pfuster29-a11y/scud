# Development Log - Scud
<!-- Registro de desarrollo (Devlog) documentado y estructurado con la asistencia de un LLM -->

# Devlog Integral - Scud 🚀

Este documento detalla el historial completo de desarrollo, decisiones de arquitectura, resolución de problemas y refactorizaciones realizadas en **Scud**, una herramienta escrita en **Rust** para automatizar, simplificar y blindar la migración y gestión de sistemas Debian hacia **Debian Sid (Unstable)**.

---

## 🛠️ Módulos y Arquitectura Implementada

### 1. Gestión Inteligente de Fuentes (`src/migrator.rs`)
* **Respaldo de Seguridad Automático:** Antes de realizar cualquier modificación, se invoca a `SystemBackup::create_sources_backup` para generar un archivo `.bak` con las fuentes originales del usuario.
* **Adaptación a Sid:** Analiza el codename de destino (`target_codename`). Si el objetivo es `sid`, implementa una lógica específica para la rama *Unstable*.
* **Manejo de Incompatibilidades:** Los repositorios de seguridad y actualizaciones (`security.debian.org`, `debian-security`, `-updates`, `/updates`) no operan de forma separada en Sid. En lugar de eliminarlos permanentemente, Scud los **comenta automáticamente** anteponiendo etiquetas identificatorias (`# [Scud] Desactivado para Sid...`). Esto evita molestos errores HTTP 404 sin destruir la configuración histórica del usuario.
* **Deduplicación por Tokens:** Utiliza un `HashSet` basado en firmas únicas de URLs y componentes (`url|components`) para evitar duplicados accidentales en el archivo resultante.

### 2. Motor de Ejecución y Comandos (`src/runner.rs`)
* **Simulación Segura (`run_apt_simulation`):** Ejecuta `apt-get dist-upgrade --just-print` para previsualizar los paquetes afectados y el resultado de la actualización de forma completamente segura y sin alterar el sistema operativo.
* **Elevación de Privilegios (`run_privileged_apt`):** Se integra con `pkexec` para manejar operaciones con permisos de superusuario.
* **Corrección Crítica de Falsos Positivos (Streaming de Terminal):**
  * *El problema:* Originalmente se utilizaba `.output()` para capturar la salida de los comandos en búferes internos. Durante migraciones masivas en Debian Sid (involucrando más de 1,400 paquetes), `apt-get` suele emitir advertencias menores en `stderr` o códigos de salida no cero que no son fatales. Esto provocaba que Scud interpretara la operación como un fallo crítico a pesar de que la actualización continuaba corriendo con éxito en segundo plano.
  * *La solución:* Se refactorizó la función para utilizar `.status()` en lugar de `.output()`. Esto permite que la salida de `apt-get` fluya directamente de manera interactiva a la consola del usuario, eliminando los falsos positivos por búferes estrictos.
* **Funciones adicionales:**
  * `run_full_audit()`: Encadena la actualización de listas de repositorios (`apt-get update`) vía privilegios seguidos de la simulación.
  * `run_apply_upgrades()`: Ejecuta la actualización real con `dist-upgrade -y`.

---

## 📌 Historial de Control de Versiones (Git)

* **Commit registrado:**
  ```bash
  git commit -m "fix(runner): usar .status() en lugar de .output() para evitar falsos positivos en comandos privilegiados de apt"

    Propósito: Cambiar el mecanismo de captura de procesos en runner.rs para garantizar la estabilidad visual y de lógica frente a actualizaciones masivas de paquetes.

🎯 Estado Actual y Próximos Pasos

    Estado actual: El núcleo funcional de migración (migrator.rs) y el motor de ejecución y comandos (runner.rs) se encuentran refactorizados y listos en el repositorio.

    Próximos pasos pendientes:

        Realizar una reinstalación limpia de Debian en el entorno de pruebas para validar de punta a punta el flujo de migración a Sid con la nueva implementación basada en streaming (.status()).

        Evaluar la integración y posible revisión del módulo complementario apt_parsers.rs para el procesamiento de datos simulados.
