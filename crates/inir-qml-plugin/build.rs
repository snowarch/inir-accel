// This code was written by: Hikari.
use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(QmlModule::new("Inir.Native").version(1, 0))
        .qt_module("Qml")
        .files(["src/lib.rs"])
        .build()
        .export();
}
