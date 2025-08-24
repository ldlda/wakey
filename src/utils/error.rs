use std::io;

enum cuh<T> {
    DNSError(io::Error),
    ToolUseError,
    Other(T),
}


impl<T> From<io::Error> for cuh<T> {
    fn from(value: io::Error) -> Self {
        todo!()
        // if matches!(value.kind(), ) {
            
        // } 
    }
}