#import <AVFoundation/AVFoundation.h>

// Return the public API's status without requesting consent from a background session.
long mr_microphone_authorization_status(void) {
    return (long)[AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
}
