//! Prepares isolated processors or adopts an exact surviving instance for a render plan.

use crate::{
    ProcessorCatalog, ProcessorControlRegistry, catalog::selected_ports,
    hosted::HostedExternalProcessor, reuse,
};
use mixrelay_audio_engine::{
    HostedAudioProcessor, HostedProcessorFactory, HostedProcessorPrepareRequest,
};

#[derive(Clone)]
pub struct ExternalProcessorFactory {
    catalog: ProcessorCatalog,
    controls: ProcessorControlRegistry,
}

struct UnavailableProcessor;

impl HostedAudioProcessor for UnavailableProcessor {
    fn process(&mut self, _input: &[f32], output: &mut [f32], _frames: usize) -> bool {
        output.fill(0.0);
        true
    }
}

impl ExternalProcessorFactory {
    #[must_use]
    pub fn new(catalog: ProcessorCatalog) -> Self {
        Self {
            catalog,
            controls: ProcessorControlRegistry::default(),
        }
    }

    #[must_use]
    pub const fn with_controls(
        catalog: ProcessorCatalog,
        controls: ProcessorControlRegistry,
    ) -> Self {
        Self { catalog, controls }
    }
}

impl HostedProcessorFactory for ExternalProcessorFactory {
    fn prepare_inactive(
        &self,
        request: HostedProcessorPrepareRequest<'_>,
    ) -> Result<Option<Box<dyn HostedAudioProcessor>>, String> {
        self.controls.verify_state(&request.config.parameters)?;
        let mut config = request.config.clone();
        if let Some(entry) = self.catalog.entry(config.type_id.as_str()) {
            config.ports = Some(selected_ports(&entry.descriptor, &config)?);
        }
        let normalized = HostedProcessorPrepareRequest {
            config: &config,
            ..request
        };
        Ok(self.controls.reuse_request(&normalized))
    }

    fn prepare(
        &self,
        request: HostedProcessorPrepareRequest<'_>,
    ) -> Result<Box<dyn HostedAudioProcessor>, String> {
        self.controls.verify_state(&request.config.parameters)?;
        let Some(entry) = self.catalog.entry(request.config.type_id.as_str()) else {
            if !request.config.ports.as_ref().is_some_and(|ports| {
                ports.total_input_channels() == Ok(request.input_channels)
                    && ports.total_output_channels() == Ok(request.output_channels)
            }) {
                return Err("unavailable processor needs its saved port contract".into());
            }
            return Ok(Box::new(UnavailableProcessor));
        };
        let selected = selected_ports(&entry.descriptor, request.config)?;
        if selected
            .total_input_channels()
            .map_err(|error| error.to_string())?
            != request.input_channels
            || selected
                .total_output_channels()
                .map_err(|error| error.to_string())?
                != request.output_channels
        {
            return Err("compiled plug-in layout does not match the catalog".to_owned());
        }
        // Reuse is tied to the resolved topology even when the authored node asks for
        // the current default. A catalog change must never inherit a same-width worker
        // whose sidechain ordering or native configuration identity has changed.
        let mut config = request.config.clone();
        config.ports = Some(selected);
        let request = HostedProcessorPrepareRequest {
            config: &config,
            ..request
        };
        if let Some(processor) = self.controls.reuse_request(&request) {
            return Ok(processor);
        }
        let (mut processor, control) = HostedExternalProcessor::new(
            &entry,
            request.config,
            request.format,
            self.controls.optional_state_store().as_ref(),
        )?;
        *control
            .reuse
            .lock()
            .map_err(|_| "processor reuse metadata is poisoned")? =
            Some(reuse::ReuseSignature::new(&request));
        processor.registration = Some(self.controls.register(request.node_id, &control));
        Ok(Box::new(processor) as Box<dyn HostedAudioProcessor>)
    }
}
