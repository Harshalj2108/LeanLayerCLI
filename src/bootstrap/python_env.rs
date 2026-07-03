use std::path::{Path, PathBuf};

use tokio::process::Command;

use crate::bootstrap::errors::BootstrapError;

pub struct PythonEnvironment {
    pub venv_path: PathBuf,
    pub python_executable: PathBuf,
}

impl PythonEnvironment {
    pub async fn ensure_initialized(app_data_dir: &Path) -> Result<Self, BootstrapError> {
        let venv_path = app_data_dir.join(".venv");
        let python_executable = venv_python(&venv_path);

        if tokio::fs::metadata(&python_executable).await.is_ok() {
            return Ok(Self {
                venv_path,
                python_executable,
            });
        }

        tokio::fs::create_dir_all(app_data_dir).await?;
        let system_python = find_python().await?;
        let output = Command::new(&system_python)
            .args(["-m", "venv"])
            .arg(&venv_path)
            .output()
            .await?;
        if !output.status.success() {
            return Err(BootstrapError::VenvCreation(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        Ok(Self {
            venv_path,
            python_executable,
        })
    }

    pub async fn sync_requirements(
        &self,
        requirements_txt_path: &Path,
    ) -> Result<(), BootstrapError> {
        if tokio::fs::metadata(requirements_txt_path).await.is_err() {
            return Ok(());
        }

        let output = Command::new(venv_pip(&self.venv_path))
            .arg("install")
            .arg("-r")
            .arg(requirements_txt_path)
            .output()
            .await?;
        if !output.status.success() {
            return Err(BootstrapError::PipError(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }
        Ok(())
    }
}

async fn find_python() -> Result<String, BootstrapError> {
    for candidate in ["python3", "python"] {
        let output = Command::new(candidate)
            .args(["-c", "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}')"])
            .output()
            .await;
        let Ok(output) = output else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let version = String::from_utf8_lossy(&output.stdout);
        let mut parts = version.trim().split('.');
        let major = parts.next().and_then(|part| part.parse::<u32>().ok());
        let minor = parts.next().and_then(|part| part.parse::<u32>().ok());
        if matches!((major, minor), (Some(major), Some(minor)) if major > 3 || (major == 3 && minor >= 10))
        {
            return Ok(candidate.to_string());
        }
    }
    Err(BootstrapError::PythonNotFound)
}

fn venv_python(venv_path: &Path) -> PathBuf {
    if cfg!(windows) {
        venv_path.join("Scripts").join("python.exe")
    } else {
        venv_path.join("bin").join("python")
    }
}

fn venv_pip(venv_path: &Path) -> PathBuf {
    if cfg!(windows) {
        venv_path.join("Scripts").join("pip.exe")
    } else {
        venv_path.join("bin").join("pip")
    }
}

#[cfg(test)]
mod tests {
    use super::PythonEnvironment;

    #[tokio::test]
    async fn ensure_initialized_creates_venv_python() {
        let dir = tempfile::tempdir().expect("tempdir");
        let env = PythonEnvironment::ensure_initialized(dir.path())
            .await
            .expect("venv");
        assert!(env.python_executable.exists());
    }
}
