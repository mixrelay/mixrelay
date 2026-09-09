// Embedded-driver tests: no installed HAL writes or real host callbacks.
static UInt64 format_requested;
static OSStatus accept_format_request(AudioServerPlugInHostRef host, AudioObjectID device,
                                     UInt64 action, void *info) {
    (void)host;
    assert(device == kObjectID_Device && info == NULL);
    format_requested = action;
    return noErr;
}

static void test_nominal_format_catalog(void) {
    AudioObjectPropertyAddress address = {kAudioDevicePropertyAvailableNominalSampleRates,
        kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
    UInt32 size = 0;
    assert(ToneNariHAL_GetDevicePropertyDataSize(gAudioServerPlugInDriverRef,
        kObjectID_Device, 0, &address, 0, NULL, &size) == noErr);
    assert(size == TN_AUDIO_SAMPLE_RATE_COUNT * sizeof(AudioValueRange));
    for (UInt32 count = 0; count <= TN_AUDIO_SAMPLE_RATE_COUNT + 1U; count++) {
        AudioValueRange ranges[TN_AUDIO_SAMPLE_RATE_COUNT + 1U];
        memset(ranges, 0xA5, sizeof(ranges));
        unsigned char before[sizeof(ranges)];
        memcpy(before, ranges, sizeof(ranges));
        UInt32 written = UINT32_MAX;
        assert(ToneNariHAL_GetDevicePropertyData(gAudioServerPlugInDriverRef, kObjectID_Device,
            0, &address, 0, NULL, count * (UInt32)sizeof(*ranges), &written, ranges) == noErr);
        UInt32 expected = count < TN_AUDIO_SAMPLE_RATE_COUNT ? count : TN_AUDIO_SAMPLE_RATE_COUNT;
        assert(written == expected * sizeof(*ranges));
        for (UInt32 i = 0; i < expected; i++) {
            assert(ranges[i].mMinimum == TN_AUDIO_SAMPLE_RATES[i]);
            assert(ranges[i].mMaximum == TN_AUDIO_SAMPLE_RATES[i]);
        }
        assert(memcmp((unsigned char *)ranges + written, before + written, sizeof(ranges) - written) == 0);
    }
}

static void test_stream_format_catalog(AudioObjectID stream, UInt32 channels) {
    const AudioObjectPropertySelector selectors[] = {
        kAudioStreamPropertyAvailableVirtualFormats, kAudioStreamPropertyAvailablePhysicalFormats};
    for (unsigned selector = 0; selector < 2U; selector++) {
        AudioObjectPropertyAddress address = {selectors[selector], kAudioObjectPropertyScopeGlobal, 0};
        UInt32 size = 0;
        assert(ToneNariHAL_GetStreamPropertyDataSize(gAudioServerPlugInDriverRef,
            stream, 0, &address, 0, NULL, &size) == noErr);
        assert(size == TN_AUDIO_SAMPLE_RATE_COUNT * sizeof(AudioStreamRangedDescription));
        for (UInt32 count = 0; count <= TN_AUDIO_SAMPLE_RATE_COUNT + 1U; count++) {
            AudioStreamRangedDescription formats[TN_AUDIO_SAMPLE_RATE_COUNT + 1U];
            memset(formats, 0xA5, sizeof(formats));
            unsigned char before[sizeof(formats)];
            memcpy(before, formats, sizeof(formats));
            UInt32 written = UINT32_MAX;
            assert(ToneNariHAL_GetStreamPropertyData(gAudioServerPlugInDriverRef, stream, 0,
                &address, 0, NULL, count * (UInt32)sizeof(*formats), &written, formats) == noErr);
            UInt32 expected = count < TN_AUDIO_SAMPLE_RATE_COUNT ? count : TN_AUDIO_SAMPLE_RATE_COUNT;
            assert(written == expected * sizeof(*formats));
            for (UInt32 i = 0; i < expected; i++) {
                assert(formats[i].mFormat.mSampleRate == TN_AUDIO_SAMPLE_RATES[i]);
                assert(formats[i].mFormat.mChannelsPerFrame == channels);
                assert(formats[i].mFormat.mBytesPerFrame == channels * sizeof(Float32));
                assert(formats[i].mSampleRateRange.mMinimum == TN_AUDIO_SAMPLE_RATES[i]);
                assert(formats[i].mSampleRateRange.mMaximum == TN_AUDIO_SAMPLE_RATES[i]);
            }
            assert(memcmp((unsigned char *)formats + written, before + written, sizeof(formats) - written) == 0);
        }
    }
}

static void test_format_changes(void) {
    AudioServerPlugInHostInterface host = {.RequestDeviceConfigurationChange = accept_format_request};
    gPlugIn_Host = &host;
    assert(tn_device_clock_initialize(&gDevice_Clock, 48000U, kDevice_RingBufferSize, 1U, 1U));
    assert(tn_hal_io_initialize(&gDevice_IOContext, 2U, 2U));
    AudioObjectPropertyAddress address = {kAudioDevicePropertyNominalSampleRate,
        kAudioObjectPropertyScopeGlobal, 0};
    AudioObjectPropertyAddress changes[2];
    UInt32 changed = 0;
    for (UInt32 i = 0; i < TN_AUDIO_SAMPLE_RATE_COUNT; i++) {
        Float64 rate = TN_AUDIO_SAMPLE_RATES[i];
        assert(ToneNariHAL_SetDevicePropertyData(gAudioServerPlugInDriverRef, kObjectID_Device,
            0, &address, 0, NULL, sizeof(rate), &rate, &changed, changes) == noErr);
        run_request();
        assert(format_requested == (UInt64)rate);
        assert(ToneNariHAL_PerformDeviceConfigurationChange(gAudioServerPlugInDriverRef,
            kObjectID_Device, format_requested, NULL) == noErr);
        assert(gDevice_SampleRate == rate && atomic_load(&gDevice_Clock.sample_rate) == (UInt32)rate);
        Float64 actual = 0;
        UInt32 size = 0;
        assert(ToneNariHAL_GetDevicePropertyData(gAudioServerPlugInDriverRef, kObjectID_Device,
            0, &address, 0, NULL, sizeof(actual), &size, &actual) == noErr);
        assert(actual == rate && size == sizeof(actual));
    }
    const Float64 invalid[] = {0, 48000.5, 352800, 384000, NAN, INFINITY};
    for (unsigned i = 0; i < sizeof(invalid) / sizeof(*invalid); i++) {
        assert(ToneNariHAL_SetDevicePropertyData(gAudioServerPlugInDriverRef, kObjectID_Device,
            0, &address, 0, NULL, sizeof(invalid[i]), &invalid[i], &changed, changes) != noErr);
        assert(queued_request == NULL && gDevice_SampleRate == 192000);
    }
    assert(ToneNariHAL_PerformDeviceConfigurationChange(gAudioServerPlugInDriverRef,
        kObjectID_Device, 384000U, NULL) != noErr);
    assert(ToneNariHAL_PerformDeviceConfigurationChange(gAudioServerPlugInDriverRef,
        kObjectID_Device, 48000U, NULL) == noErr);
    gPlugIn_Host = NULL;
}

static void test_all_formats(void) {
    test_nominal_format_catalog();
    const UInt32 channels[] = {1U, 2U, 7U, 64U};
    for (unsigned i = 0; i < sizeof(channels) / sizeof(*channels); i++) {
        gDevice_RenderChannels = channels[i];
        gDevice_CaptureChannels = channels[i];
        test_stream_format_catalog(kObjectID_Stream_Input, channels[i]);
        test_stream_format_catalog(kObjectID_Stream_Output, channels[i]);
    }
    gDevice_RenderChannels = gDevice_CaptureChannels = 2U;
    test_format_changes();
    puts("HAL format checks: six rates, both stream directions, 1/2/7/64 channels, partial reads, native rate/clock changes, invalid rejection");
}
