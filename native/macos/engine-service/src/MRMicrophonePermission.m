#import <AVFoundation/AVFoundation.h>

// Return the public API's status without requesting consent from a background session.
long mr_microphone_authorization_status(void) {
    return (long)[AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
}

// Request asynchronously; Rust observes the decision with a bounded control-side wait.
void mr_microphone_request_authorization(void) {
    [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio completionHandler:^(BOOL granted) {
        (void)granted;
    }];
}
