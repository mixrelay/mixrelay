// Test the real listener's SIGTERM cleanup in a temporary user launchd job.
// No system job, authorization request or installed Driver file is touched.
#import "../src/removal/native.m"
#include <unistd.h>

static const char *executable;
static const char *evidence;
static int reject(bool selected) { (void)selected; return 1; }
static void cleanup(void) {
    unlink(executable);
    [@"idle cleanup observed" writeToFile:[NSString stringWithUTF8String:evidence]
        atomically:YES encoding:NSUTF8StringEncoding error:NULL];
    exit(0);
}

int main(int argc, char **argv) {
    if (argc != 4) return 2;
    executable = argv[1]; evidence = argv[2];
    const char *ready = argv[3];
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, NSEC_PER_SEC / 10), dispatch_get_global_queue(QOS_CLASS_DEFAULT, 0), ^{
        [@"listener ready" writeToFile:[NSString stringWithUTF8String:ready]
            atomically:YES encoding:NSUTF8StringEncoding error:NULL];
    });
    return MRRemovalServer("AB12C3D4E5", reject, cleanup, cleanup);
}
