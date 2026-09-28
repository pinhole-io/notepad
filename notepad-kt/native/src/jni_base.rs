//! Shared helpers for JNI modules.

use std::sync::OnceLock;

use jni::JNIEnv;
use jni::JavaVM;
use jni::sys::jstring;
use tokio::runtime::Runtime;

pub static RT: OnceLock<Runtime> = OnceLock::new();
pub static JVM: OnceLock<std::sync::Arc<JavaVM>> = OnceLock::new();

pub fn runtime() -> &'static Runtime {
    RT.get_or_init(|| Runtime::new().expect("tokio runtime"))
}

pub fn ensure_jvm(env: &mut JNIEnv) -> std::sync::Arc<JavaVM> {
    JVM.get_or_init(|| {
        let vm = env.get_java_vm().expect("get_java_vm");
        std::sync::Arc::new(vm)
    })
    .clone()
}

pub fn jthrow(env: &mut JNIEnv, msg: &str) {
    let _ = env.throw_new("java/lang/RuntimeException", msg);
}

pub fn jstr_from_str(env: &mut JNIEnv, s: &str) -> jstring {
    match env.new_string(s) {
        Ok(js) => js.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

pub fn jstr_to_string(env: &mut JNIEnv, j: jstring) -> Result<String, String> {
    if j.is_null() {
        return Ok(String::new());
    }
    let s = unsafe { jni::objects::JString::from_raw(j) };
    env.get_string(&s)
        .map(|x| String::from(x))
        .map_err(|e| e.to_string())
}
