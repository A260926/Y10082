//! Metric registry implementation.
//!
//! See [`Registry`] for details.

use std::borrow::Cow;

use crate::collector::Collector;
use crate::encoding::{DescriptorEncoder, EncodeMetric};
use crate::metrics::MetricType;

/// A metric registry.
///
/// First off one registers metrics with the registry via
/// [`Registry::register`]. Later on the [`Registry`] is passed to an encoder
/// collecting samples of each metric by iterating all metrics in the
/// [`Registry`].
///
/// [`Registry`] is the core building block, generic over the metric type being
/// registered. Out of convenience, the generic type parameter is set to use
/// dynamic dispatching by default to be able to register different types of
/// metrics (e.g. [`Counter`](crate::metrics::counter::Counter) and
/// [`Gauge`](crate::metrics::gauge::Gauge)) with the same registry. Advanced
/// users might want to use their custom types.
///
/// ```
/// # use prometheus_client::encoding::text::encode;
/// # use prometheus_client::metrics::counter::{Atomic as _, Counter};
/// # use prometheus_client::metrics::gauge::{Atomic as _, Gauge};
/// # use prometheus_client::registry::Registry;
/// #
/// // Create a metric registry.
/// let mut registry = Registry::default();
///
/// let counter: Counter = Counter::<u64>::default();
/// let gauge: Gauge = Gauge::<i64>::default();
///
/// registry.register(
///   "my_counter",
///   "This is my counter",
///   counter.clone(),
/// );
/// registry.register(
///   "my_gauge",
///   "This is my gauge",
///   gauge.clone(),
/// );
///
/// # // Encode all metrics in the registry in the text format.
/// # let mut buffer = String::new();
/// # encode(&mut buffer, &registry).unwrap();
/// #
/// # let expected = "# HELP my_counter This is my counter.\n".to_owned() +
/// #                "# TYPE my_counter counter\n" +
/// #                "my_counter_total 0\n" +
/// #                "# HELP my_gauge This is my gauge.\n" +
/// #                "# TYPE my_gauge gauge\n" +
/// #                "my_gauge 0\n" +
/// #                "# EOF\n";
/// # assert_eq!(expected, buffer);
/// ```
#[derive(Debug, Default)]
pub struct Registry {
    prefix: Option<Prefix>,
    labels: Vec<(Cow<'static, str>, Cow<'static, str>)>,
    metrics: Vec<(Descriptor, Box<dyn Metric>)>,
    collectors: Vec<Box<dyn Collector>>,
    sub_registries: Vec<Registry>,
}

impl Registry {
    /// Creates a new default [`Registry`] with the given prefix.
    pub fn with_prefix(prefix: impl Into<String>) -> Self {
        Self {
            prefix: Some(Prefix(prefix.into())),
            ..Default::default()
        }
    }

    /// Creates a new default [`Registry`] with the given labels.
    pub fn with_labels(
        labels: impl Iterator<Item = (Cow<'static, str>, Cow<'static, str>)>,
    ) -> Self {
        Self {
            labels: labels.into_iter().collect(),
            ..Default::default()
        }
    }

    /// Creates a new default [`Registry`] with the given prefix and labels.
    pub fn with_prefix_and_labels(
        prefix: impl Into<String>,
        labels: impl Iterator<Item = (Cow<'static, str>, Cow<'static, str>)>,
    ) -> Self {
        Self {
            prefix: Some(Prefix(prefix.into())),
            labels: labels.into_iter().collect(),
            ..Default::default()
        }
    }

    /// Register a metric with the [`Registry`].
    ///
    /// Note: In the Open Metrics text exposition format some metric types have
    /// a special suffix, e.g. the
    /// [`Counter`](crate::metrics::counter::Counter`) metric with `_total`.
    /// These suffixes are inferred through the metric type and must not be
    /// appended to the metric name manually by the user.
    ///
    /// Note: A full stop punctuation mark (`.`) is automatically added to the
    /// passed help text.
    ///
    /// Use [`Registry::register_with_unit`] whenever a unit for the given
    /// metric is known.
    ///
    /// ```
    /// # use prometheus_client::metrics::counter::{Atomic as _, Counter};
    /// # use prometheus_client::registry::{Registry, Unit};
    /// #
    /// let mut registry = Registry::default();
    /// let counter: Counter = Counter::<u64>::default();
    ///
    /// registry.register("my_counter", "This is my counter", counter.clone());
    /// ```
    pub fn register<N: Into<String>, H: Into<String>>(
        &mut self,
        name: N,
        help: H,
        metric: impl Metric,
    ) {
        self.priv_register(name, help, metric, None)
    }

    /// Register a metric with the [`Registry`] specifying the metric's unit.
    ///
    /// See [`Registry::register`] for additional documentation.
    ///
    /// Note: In the Open Metrics text exposition format units are appended to
    /// the metric name. This is done automatically. Users must not append the
    /// unit to the name manually.
    ///
    /// ```
    /// # use prometheus_client::metrics::counter::{Atomic as _, Counter};
    /// # use prometheus_client::registry::{Registry, Unit};
    /// #
    /// let mut registry = Registry::default();
    /// let counter: Counter = Counter::<u64>::default();
    ///
    /// registry.register_with_unit(
    ///   "my_counter",
    ///   "This is my counter",
    ///   Unit::Seconds,
    ///   counter.clone(),
    /// );
    /// ```
    pub fn register_with_unit<N: Into<String>, H: Into<String>>(
        &mut self,
        name: N,
        help: H,
        unit: Unit,
        metric: impl Metric,
    ) {
        self.priv_register(name, help, metric, Some(unit))
    }

    fn priv_register<N: Into<String>, H: Into<String>>(
        &mut self,
        name: N,
        help: H,
        metric: impl Metric,
        unit: Option<Unit>,
    ) {
        let descriptor = Descriptor::new(name, help, unit);
        self.metrics.push((descriptor, Box::new(metric)));
    }

    /// Register a [`Collector`].
    ///
    /// ```
    /// # use prometheus_client::metrics::counter::ConstCounter;
    /// # use prometheus_client::registry::Registry;
    /// # use prometheus_client::collector::Collector;
    /// # use prometheus_client::encoding::{DescriptorEncoder, EncodeMetric};
    /// #
    /// #[derive(Debug)]
    /// struct MyCollector {}
    ///
    /// impl Collector for MyCollector {
    ///     fn encode(&self, mut encoder: DescriptorEncoder) -> Result<(), std::fmt::Error> {
    ///         let counter = ConstCounter::new(42u64);
    ///         let metric_encoder = encoder.encode_descriptor(
    ///             "my_counter",
    ///             "some help",
    ///             None,
    ///             counter.metric_type(),
    ///         )?;
    ///         counter.encode(metric_encoder)?;
    ///         Ok(())
    ///     }
    /// }
    ///
    /// let my_collector = Box::new(MyCollector{});
    ///
    /// let mut registry = Registry::default();
    ///
    /// registry.register_collector(my_collector);
    /// ```
    pub fn register_collector(&mut self, collector: Box<dyn Collector>) {
        self.collectors.push(collector);
    }

    /// Create a sub-registry to register metrics with a common prefix.
    ///
    /// Say you would like to prefix one set of metrics with `subsystem_a` and
    /// one set of metrics with `subsystem_b`. Instead of prefixing each metric
    /// with the corresponding subsystem string individually, you can create two
    /// sub-registries like demonstrated below.
    ///
    /// This can be used to pass a prefixed sub-registry down to a subsystem of
    /// your architecture automatically adding a prefix to each metric the
    /// subsystem registers.
    ///
    /// ```
    /// # use prometheus_client::metrics::counter::{Atomic as _, Counter};
    /// # use prometheus_client::registry::{Registry, Unit};
    /// #
    /// let mut registry = Registry::default();
    ///
    /// let subsystem_a_counter_1: Counter = Counter::<u64>::default();
    /// let subsystem_a_counter_2: Counter = Counter::<u64>::default();
    ///
    /// let subsystem_a_registry = registry.sub_registry_with_prefix("subsystem_a");
    /// subsystem_a_registry.register("counter_1", "", subsystem_a_counter_1.clone());
    /// subsystem_a_registry.register("counter_2", "", subsystem_a_counter_2.clone());
    ///
    /// let subsystem_b_counter_1: Counter = Counter::<u64>::default();
    /// let subsystem_b_counter_2: Counter = Counter::<u64>::default();
    ///
    /// let subsystem_b_registry = registry.sub_registry_with_prefix("subsystem_b");
    /// subsystem_b_registry.register("counter_1", "", subsystem_b_counter_1.clone());
    /// subsystem_b_registry.register("counter_2", "", subsystem_b_counter_2.clone());
    /// ```
    ///
    /// See [`Registry::sub_registry_with_label`] for the same functionality,
    /// but namespacing with a label instead of a metric name prefix.
    pub fn sub_registry_with_prefix<P: AsRef<str>>(&mut self, prefix: P) -> &mut Self {
        let sub_registry = Registry {
            prefix: Some(Prefix(
                self.prefix.clone().map(|p| p.0 + "_").unwrap_or_default() + prefix.as_ref(),
            )),
            labels: self.labels.clone(),
            ..Default::default()
        };

        self.priv_sub_registry(sub_registry)
    }

    /// Like [`Registry::sub_registry_with_prefix`] but with a label instead.
    pub fn sub_registry_with_label(
        &mut self,
        label: (Cow<'static, str>, Cow<'static, str>),
    ) -> &mut Self {
        self.sub_registry_with_labels(std::iter::once(label))
    }

    /// Like [`Registry::sub_registry_with_prefix`] but with multiple labels instead.
    pub fn sub_registry_with_labels(
        &mut self,
        labels: impl Iterator<Item = (Cow<'static, str>, Cow<'static, str>)>,
    ) -> &mut Self {
        let mut new_labels = self.labels.clone();
        new_labels.extend(labels);

        let sub_registry = Registry {
            prefix: self.prefix.clone(),
            labels: new_labels,
            ..Default::default()
        };

        self.priv_sub_registry(sub_registry)
    }

    fn priv_sub_registry(&mut self, sub_registry: Self) -> &mut Self {
        self.sub_registries.push(sub_registry);

        self.sub_registries
            .last_mut()
            .expect("sub_registries not to be empty.")
    }

    /// Create a composable, read-only view of this [`Registry`].
    ///
    /// The returned [`RegistryView`] does not copy any metric objects and does
    /// not alter the registration order of the registry. It can be restricted
    /// to a subset of the metric families by stacking name prefix, full metric
    /// name and const label conditions, see [`RegistryView`] for details. A
    /// view without any conditions encodes the same families as the registry
    /// itself.
    ///
    /// The view is evaluated against the registry at encode time, i.e. metrics
    /// registered after the view was created are still included as long as they
    /// satisfy the view's conditions.
    ///
    /// ```
    /// # use prometheus_client::encoding::text::encode;
    /// # use prometheus_client::metrics::counter::{Atomic as _, Counter};
    /// # use prometheus_client::registry::Registry;
    /// #
    /// let mut registry = Registry::default();
    ///
    /// let api_requests: Counter = Counter::<u64>::default();
    /// registry.register("api_requests", "API requests", api_requests.clone());
    ///
    /// let ui_requests: Counter = Counter::<u64>::default();
    /// registry.register("ui_requests", "UI requests", ui_requests.clone());
    ///
    /// // A view only exposing families whose name starts with "api_".
    /// let view = registry.view().with_name_prefix("api");
    ///
    /// let mut buffer = String::new();
    /// encode(&mut buffer, &view).unwrap();
    ///
    /// assert!(buffer.contains("api_requests_total"));
    /// assert!(!buffer.contains("ui_requests_total"));
    /// assert!(buffer.contains("# EOF\n"));
    /// ```
    pub fn view(&self) -> RegistryView<'_> {
        RegistryView {
            registry: self,
            filter: ViewFilter::default(),
        }
    }

    /// Collect a stable, depth-first snapshot of the entries to encode.
    ///
    /// All descriptors that can become part of the output are gathered before
    /// any byte or protobuf family is emitted. Registrations happening while
    /// the returned snapshot is iterated (e.g. concurrently) therefore can not
    /// show up in the middle of an exposition; they are picked up by the next
    /// encode instead.
    fn collect<'a>(
        &'a self,
        entries: &mut Vec<PlannedEntry<'a>>,
        prefix: Option<&'a Prefix>,
        labels: &'a [(Cow<'static, str>, Cow<'static, str>)],
        filter: Option<&'a ViewFilter>,
    ) {
        entries.reserve(self.metrics.len() + self.collectors.len() + self.sub_registries.len());

        for (descriptor, metric) in self.metrics.iter() {
            if metric.is_empty() {
                continue;
            }

            if let Some(filter) = filter {
                let full_name = metric_family_name(
                    prefix,
                    &descriptor.name,
                    descriptor.unit.as_ref(),
                    EncodeMetric::metric_type(metric.as_ref()),
                );
                if !filter.matches(&full_name, labels) {
                    continue;
                }
            }

            entries.push(PlannedEntry::Metric {
                prefix,
                labels,
                descriptor,
                metric: metric.as_ref(),
            });
        }

        for collector in self.collectors.iter() {
            entries.push(PlannedEntry::Collector {
                prefix,
                labels,
                collector: collector.as_ref(),
            });
        }

        for sub_registry in self.sub_registries.iter() {
            sub_registry.collect(
                entries,
                sub_registry.prefix.as_ref(),
                &sub_registry.labels,
                filter,
            );
        }
    }
}

/// Anything that can be encoded as an OpenMetrics exposition, i.e. a
/// [`Registry`] itself or a filtered [`RegistryView`] of one.
///
/// This trait is implemented for both types and allows the text and protobuf
/// encoders to be used with either of them. It is [sealed] and can not be
/// implemented for any other type.
///
/// [sealed]: https://rust-lang.github.io/api-guidelines/future-proofing.html#sealed-traits-protect-against-internal-changes-c-sealed
pub trait EncodeRegistry: private::Sealed {
    /// Encode the selected metric families into the given descriptor encoder.
    #[doc(hidden)]
    fn encode_into(&self, encoder: &mut DescriptorEncoder) -> Result<(), std::fmt::Error>;
}

mod private {
    pub trait Sealed {}
}

/// A single descriptor selected for an exposition.
enum PlannedEntry<'a> {
    Metric {
        prefix: Option<&'a Prefix>,
        labels: &'a [(Cow<'static, str>, Cow<'static, str>)],
        descriptor: &'a Descriptor,
        metric: &'a dyn Metric,
    },
    Collector {
        prefix: Option<&'a Prefix>,
        labels: &'a [(Cow<'static, str>, Cow<'static, str>)],
        collector: &'a dyn Collector,
    },
}

/// Encode a pre-collected, stable list of entries.
///
/// The set and order of the families are already fixed by the previously
/// collected entries. The [`DescriptorEncoder`] additionally carries the
/// optional [`ViewFilter`] so that dynamically produced collector families are
/// gated by the exact same rules. Both the text and the protobuf encoders go
/// through this single path and thus can not diverge in the selected family
/// set or order.
fn encode_entries<'a>(
    entries: impl IntoIterator<Item = PlannedEntry<'a>>,
    encoder: &mut DescriptorEncoder,
) -> Result<(), std::fmt::Error> {
    for entry in entries {
        match entry {
            PlannedEntry::Metric {
                prefix,
                labels,
                descriptor,
                metric,
            } => {
                let mut descriptor_encoder = encoder.with_prefix_and_labels(prefix, labels);
                let metric_encoder = descriptor_encoder.encode_descriptor(
                    &descriptor.name,
                    &descriptor.help,
                    descriptor.unit.as_ref(),
                    EncodeMetric::metric_type(metric),
                )?;
                metric.encode(metric_encoder)?;
            }
            PlannedEntry::Collector {
                prefix,
                labels,
                collector,
            } => {
                let descriptor_encoder = encoder.with_prefix_and_labels(prefix, labels);
                collector.encode(descriptor_encoder)?;
            }
        }
    }

    Ok(())
}

impl private::Sealed for Registry {}

impl EncodeRegistry for Registry {
    fn encode_into(&self, encoder: &mut DescriptorEncoder) -> Result<(), std::fmt::Error> {
        let mut entries = Vec::new();
        self.collect(&mut entries, self.prefix.as_ref(), &self.labels, None);
        encode_entries(entries, encoder)
    }
}

/// A composable, read-only view of a [`Registry`].
///
/// A `RegistryView` exposes only the metric families matching all of the
/// conditions stacked onto it. Views borrow the registry they are built from,
/// never copy metric objects and preserve the registry's registration order.
/// Conditions can be combined freely:
///
/// - [`RegistryView::with_name_prefix`] keeps families whose final public
///   metric name starts with the given prefix followed by a name boundary
///   (`_` or the end of the name), so `api` does not match `apiary`.
/// - [`RegistryView::with_full_name`] keeps only the family with exactly the
///   given final public metric name.
/// - [`RegistryView::with_const_label`] and
///   [`RegistryView::with_const_labels`] keep families carrying the given
///   const label key/value pair(s), expanded with all prefix and label rules
///   of the sub-registry they were registered in.
///
/// Stacking the same kind of condition repeatedly takes the intersection,
/// e.g. two prefixes only match names satisfying both. A view without any
/// condition is equivalent to encoding the underlying registry.
///
/// Conditions are evaluated against the final, public metric name, i.e. the
/// name written after `# TYPE` in the OpenMetrics text format, including
/// registry prefixes and unit suffixes and the type-specific `_total` /
/// `_info` suffixes.
///
/// A view does not snapshot the registry: metrics registered after the view
/// was created are visible on the next encode as long as they match. Each
/// individual encode, however, uses the stable set of descriptors present
/// when the encode started; families registered during an exposition only
/// appear in the following one.
///
/// ```
/// # use prometheus_client::encoding::text::encode;
/// # use prometheus_client::metrics::counter::{Atomic as _, Counter};
/// # use prometheus_client::registry::Registry;
/// # use std::borrow::Cow;
/// #
/// let mut registry = Registry::default();
/// let requests: Counter = Counter::<u64>::default();
/// registry.sub_registry_with_prefix("api")
///     .register("requests", "Requests", requests.clone());
///
/// // Only families prefixed with "api_" and carrying the const label
/// // `service="billing"`.
/// let view = registry
///     .view()
///     .with_name_prefix("api")
///     .with_const_label((Cow::Borrowed("service"), Cow::Borrowed("billing")));
///
/// let mut buffer = String::new();
/// encode(&mut buffer, &view).unwrap();
/// ```
#[derive(Debug)]
pub struct RegistryView<'a> {
    registry: &'a Registry,
    filter: ViewFilter,
}

impl<'a> RegistryView<'a> {
    /// Restrict the view to families whose final public metric name starts
    /// with `prefix` followed by a `_` boundary or matching it exactly.
    ///
    /// For example, the prefix `api` matches `api_requests_total` but not
    /// `apiary_total`.
    ///
    /// ```
    /// # use prometheus_client::encoding::text::encode;
    /// # use prometheus_client::metrics::counter::{Atomic as _, Counter};
    /// # use prometheus_client::registry::Registry;
    /// #
    /// let mut registry = Registry::default();
    /// let a: Counter = Counter::<u64>::default();
    /// let b: Counter = Counter::<u64>::default();
    /// registry.register("api_requests", "", a.clone());
    /// registry.register("apiary_count", "", b.clone());
    ///
    /// let mut buffer = String::new();
    /// encode(&mut buffer, &registry.view().with_name_prefix("api")).unwrap();
    ///
    /// assert!(buffer.contains("api_requests_total"));
    /// assert!(!buffer.contains("apiary_total"));
    /// ```
    pub fn with_name_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.filter.prefixes.push(prefix.into());
        self
    }

    /// Restrict the view to the family whose final public metric name equals
    /// `name` exactly.
    ///
    /// The name must be the full, public metric name, including registry
    /// prefixes, unit suffixes and type-specific suffixes such as the
    /// counter's `_total`.
    ///
    /// ```
    /// # use prometheus_client::encoding::text::encode;
    /// # use prometheus_client::metrics::counter::{Atomic as _, Counter};
    /// # use prometheus_client::registry::Registry;
    /// #
    /// let mut registry = Registry::default();
    /// let a: Counter = Counter::<u64>::default();
    /// let b: Counter = Counter::<u64>::default();
    /// registry.register("api_requests", "", a.clone());
    /// registry.register("api_failures", "", b.clone());
    ///
    /// let mut buffer = String::new();
    /// encode(&mut buffer, &registry.view().with_full_name("api_requests_total")).unwrap();
    ///
    /// assert!(buffer.contains("api_requests_total"));
    /// assert!(!buffer.contains("api_failures_total"));
    /// ```
    pub fn with_full_name(mut self, name: impl Into<String>) -> Self {
        self.filter.names.push(name.into());
        self
    }

    /// Restrict the view to families carrying the given const label.
    ///
    /// The pair is compared against the const labels of the (sub-)registry
    /// the family was registered in after they have been expanded with the
    /// existing prefix and label rules. Calling this method repeatedly
    /// requires all given pairs to be present; label declaration order does
    /// not matter.
    ///
    /// ```
    /// # use prometheus_client::encoding::text::encode;
    /// # use prometheus_client::metrics::counter::{Atomic as _, Counter};
    /// # use prometheus_client::registry::Registry;
    /// # use std::borrow::Cow;
    /// #
    /// let mut registry = Registry::default();
    /// let a: Counter = Counter::<u64>::default();
    /// let b: Counter = Counter::<u64>::default();
    /// registry
    ///     .sub_registry_with_label((Cow::Borrowed("tenant"), Cow::Borrowed("a")))
    ///     .register("requests", "", a.clone());
    /// registry
    ///     .sub_registry_with_label((Cow::Borrowed("tenant"), Cow::Borrowed("b")))
    ///     .register("requests", "", b.clone());
    ///
    /// let view = registry
    ///     .view()
    ///     .with_const_label((Cow::Borrowed("tenant"), Cow::Borrowed("a")));
    ///
    /// let mut buffer = String::new();
    /// encode(&mut buffer, &view).unwrap();
    ///
    /// assert!(buffer.contains(r#"tenant="a""#));
    /// assert!(!buffer.contains(r#"tenant="b""#));
    /// ```
    pub fn with_const_label(mut self, label: (Cow<'static, str>, Cow<'static, str>)) -> Self {
        self.filter.required_labels.push(label);
        self
    }

    /// Restrict the view to families carrying all of the given const labels.
    ///
    /// This is the batch variant of [`RegistryView::with_const_label`] and
    /// can be combined with it and with itself.
    ///
    /// ```
    /// # use prometheus_client::registry::Registry;
    /// # use std::borrow::Cow;
    /// #
    /// let mut registry = Registry::default();
    /// let view = registry.view().with_const_labels(
    ///     [
    ///         (Cow::Borrowed("tenant"), Cow::Borrowed("team")),
    ///         (Cow::Borrowed("env"), Cow::Borrowed("prod")),
    ///     ]
    ///     .into_iter(),
    /// );
    /// ```
    pub fn with_const_labels(
        mut self,
        labels: impl Iterator<Item = (Cow<'static, str>, Cow<'static, str>)>,
    ) -> Self {
        self.filter.required_labels.extend(labels);
        self
    }
}

impl private::Sealed for RegistryView<'_> {}

impl EncodeRegistry for RegistryView<'_> {
    fn encode_into(&self, encoder: &mut DescriptorEncoder) -> Result<(), std::fmt::Error> {
        encoder.set_filter(self.filter.clone());

        let mut entries = Vec::new();
        self.registry.collect(
            &mut entries,
            self.registry.prefix.as_ref(),
            &self.registry.labels,
            Some(&self.filter),
        );
        encode_entries(entries, encoder)
    }
}

/// A shared [`Registry`] can be encoded directly, as used by several HTTP
/// server integrations.
impl private::Sealed for std::sync::Arc<Registry> {}

impl EncodeRegistry for std::sync::Arc<Registry> {
    fn encode_into(&self, encoder: &mut DescriptorEncoder) -> Result<(), std::fmt::Error> {
        EncodeRegistry::encode_into(self.as_ref(), encoder)
    }
}

/// Conjunction of all view conditions. An empty filter matches every family.
#[derive(Clone, Debug, Default)]
pub(crate) struct ViewFilter {
    /// Every given prefix must match the final public metric name.
    pub(crate) prefixes: Vec<String>,
    /// Every given name must equal the final public metric name.
    pub(crate) names: Vec<String>,
    /// Every given pair must be present among the expanded const labels.
    pub(crate) required_labels: Vec<(Cow<'static, str>, Cow<'static, str>)>,
}

impl ViewFilter {
    pub(crate) fn matches(
        &self,
        full_name: &str,
        const_labels: &[(Cow<'static, str>, Cow<'static, str>)],
    ) -> bool {
        self.prefixes
            .iter()
            .all(|prefix| has_name_prefix(full_name, prefix))
            && self.names.iter().all(|name| name == full_name)
            && self
                .required_labels
                .iter()
                .all(|required| const_labels.iter().any(|label| label == required))
    }
}

/// Check whether `name` starts with `prefix` on a name boundary.
///
/// Names are joined with `_`, hence the prefix either equals the name or is
/// followed by an underscore. This keeps `api` from matching `apiary`.
fn has_name_prefix(name: &str, prefix: &str) -> bool {
    match name.strip_prefix(prefix) {
        Some(rest) => rest.is_empty() || rest.starts_with('_'),
        None => false,
    }
}

/// Build the final public metric family name as emitted by the encoders.
///
/// Prefix and unit are joined with `_`; counters and infos carry their
/// type-specific `_total` and `_info` suffixes. Both the text encoder (in the
/// `# TYPE` line) and the protobuf encoders follow these same rules.
pub(crate) fn metric_family_name(
    prefix: Option<&Prefix>,
    name: &str,
    unit: Option<&Unit>,
    metric_type: MetricType,
) -> String {
    let mut full_name = String::new();

    if let Some(prefix) = prefix {
        full_name.push_str(prefix.as_str());
        full_name.push('_');
    }

    full_name.push_str(name);

    if let Some(unit) = unit {
        full_name.push('_');
        full_name.push_str(unit.as_str());
    }

    match metric_type {
        MetricType::Counter => full_name.push_str("_total"),
        MetricType::Info => full_name.push_str("_info"),
        MetricType::Gauge | MetricType::Histogram | MetricType::Unknown => {}
    }

    full_name
}

/// Metric prefix
#[derive(Clone, Debug)]
pub(crate) struct Prefix(String);

impl Prefix {
    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<String> for Prefix {
    fn from(s: String) -> Self {
        Prefix(s)
    }
}

/// OpenMetrics metric descriptor.
#[derive(Debug, Clone)]
struct Descriptor {
    name: String,
    help: String,
    unit: Option<Unit>,
}

impl Descriptor {
    /// Create new [`Descriptor`].
    fn new<N: Into<String>, H: Into<String>>(name: N, help: H, unit: Option<Unit>) -> Self {
        Self {
            name: name.into(),
            help: help.into() + ".",
            unit,
        }
    }
}

/// Metric units recommended by Open Metrics.
///
/// See [`Unit::Other`] to specify alternative units.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub enum Unit {
    Amperes,
    Bytes,
    Celsius,
    Grams,
    Joules,
    Meters,
    Ratios,
    Seconds,
    Volts,
    Other(String),
}

impl Unit {
    /// Returns the given Unit's str representation.
    pub fn as_str(&self) -> &str {
        match self {
            Unit::Amperes => "amperes",
            Unit::Bytes => "bytes",
            Unit::Celsius => "celsius",
            Unit::Grams => "grams",
            Unit::Joules => "joules",
            Unit::Meters => "meters",
            Unit::Ratios => "ratios",
            Unit::Seconds => "seconds",
            Unit::Volts => "volts",
            Unit::Other(other) => other.as_str(),
        }
    }
}

/// Super trait representing an abstract Prometheus metric.
pub trait Metric: crate::encoding::EncodeMetric + Send + Sync + std::fmt::Debug + 'static {}

impl<T> Metric for T where T: crate::encoding::EncodeMetric + Send + Sync + std::fmt::Debug + 'static
{}

#[cfg(test)]
mod view_tests {
    use super::*;
    use crate::encoding::text::encode;
    use crate::metrics::counter::Counter;
    use crate::metrics::gauge::Gauge;

    /// Extract the metric family names written after `# TYPE ` in text
    /// exposition, in encounter order.
    fn text_family_names(output: &str) -> Vec<&str> {
        output
            .lines()
            .filter_map(|line| line.strip_prefix("# TYPE "))
            .map(|line| line.split(' ').next().unwrap())
            .collect()
    }

    fn assert_no_half_descriptors(output: &str) {
        for line in output.lines() {
            if line.starts_with("# HELP ") || line.starts_with("# UNIT ") {
                let name = line.split(' ').nth(2).unwrap();
                assert!(
                    output.contains(&format!("# TYPE {name}")),
                    "HELP/UNIT line without matching TYPE for {name}"
                );
            }
        }
    }

    #[test]
    fn registry_view_nested_sub_registries() {
        let mut registry = Registry::default();

        let top: Counter = Counter::<u64>::default();
        registry.register("top_requests", "top", top);

        // Nested prefixes and labels must be expanded per existing rules.
        let sub = registry.sub_registry_with_prefix("tenant_a");
        let sub_sub = sub
            .sub_registry_with_prefix("api")
            .sub_registry_with_label((Cow::Borrowed("env"), Cow::Borrowed("prod")));
        let nested: Counter = Counter::<u64>::default();
        sub_sub.register("requests", "nested", nested.clone());

        // A neighbor with a different label value.
        let other = registry
            .sub_registry_with_prefix("tenant_a")
            .sub_registry_with_prefix("api")
            .sub_registry_with_label((Cow::Borrowed("env"), Cow::Borrowed("staging")));
        other.register("requests", "other", Counter::<u64>::default());

        let view = registry
            .view()
            .with_name_prefix("tenant_a_api")
            .with_const_label((Cow::Borrowed("env"), Cow::Borrowed("prod")));

        let mut output = String::new();
        encode(&mut output, &view).unwrap();

        assert_eq!(text_family_names(&output), vec!["tenant_a_api_requests"]);
        assert!(output.contains(r#"tenant_a_api_requests_total{env="prod"}"#));
        assert!(!output.contains("staging"));
        assert!(!output.contains("top_requests"));
        assert!(output.ends_with("# EOF\n"));
        assert_no_half_descriptors(&output);
    }

    #[test]
    fn registry_view_prefix_boundary() {
        let mut registry = Registry::default();
        let api: Gauge = Gauge::<i64>::default();
        let api_requests: Gauge = Gauge::<i64>::default();
        let apiary: Gauge = Gauge::<i64>::default();
        registry.register("api", "api", api);
        registry.register("api_requests", "api requests", api_requests);
        registry.register("apiary_count", "apiary", apiary);

        let mut output = String::new();
        encode(&mut output, &registry.view().with_name_prefix("api")).unwrap();

        let names = text_family_names(&output);
        assert_eq!(names, vec!["api", "api_requests"]);
        assert!(!names.contains(&"apiary_count"));
        assert!(output.ends_with("# EOF\n"));
        assert_no_half_descriptors(&output);
    }

    #[test]
    fn registry_view_prefixes_intersect() {
        let mut registry = Registry::default();
        registry.register("a_b_c", "", Gauge::<i64>::default());
        registry.register("a_b_x", "", Gauge::<i64>::default());
        registry.register("a_x_c", "", Gauge::<i64>::default());

        let view = registry
            .view()
            .with_name_prefix("a")
            .with_name_prefix("a_b");

        let mut output = String::new();
        encode(&mut output, &view).unwrap();
        assert_eq!(text_family_names(&output), vec!["a_b_c", "a_b_x"]);
    }

    #[test]
    fn registry_view_full_name_includes_suffixes() {
        let mut registry = Registry::default();
        registry.sub_registry_with_prefix("svc").register(
            "requests",
            "",
            Counter::<u64>::default(),
        );
        registry.register("svc_failures", "", Counter::<u64>::default());

        // Match against the final public name including the counter suffix.
        let view = registry.view().with_full_name("svc_requests_total");

        let mut output = String::new();
        encode(&mut output, &view).unwrap();
        assert_eq!(text_family_names(&output), vec!["svc_requests"]);
        assert!(output.contains("svc_requests_total"));
        assert!(!output.contains("svc_failures_total"));
    }

    #[test]
    fn registry_view_prefix_with_unit_suffix() {
        use crate::metrics::histogram::{exponential_buckets, Histogram};

        let mut registry = Registry::default();
        let request_duration = Histogram::new(exponential_buckets(0.005, 2.0, 10));
        registry.register_with_unit(
            "request_duration",
            "Request duration",
            Unit::Seconds,
            request_duration,
        );
        registry.register("other_gauge", "", Gauge::<i64>::default());

        // Matching uses the final public family name with the unit suffix.
        let view = registry.view().with_full_name("request_duration_seconds");

        let mut output = String::new();
        encode(&mut output, &view).unwrap();

        assert!(output.contains("# HELP request_duration_seconds"));
        assert!(output.contains("# TYPE request_duration_seconds histogram"));
        assert!(output.contains("# UNIT request_duration_seconds seconds"));
        assert!(output.contains("request_duration_seconds_sum"));
        assert!(!output.contains("other_gauge"));
        assert!(output.ends_with("# EOF\n"));
        assert_no_half_descriptors(&output);
    }

    #[test]
    fn registry_view_info_suffix_and_label_filter() {
        use crate::metrics::info::Info;

        let mut registry = Registry::default();
        registry
            .sub_registry_with_label((Cow::Borrowed("component"), Cow::Borrowed("auth")))
            .register(
                "build",
                "",
                Info::new(vec![("version".to_string(), "1.0".to_string())]),
            );

        // Info metrics are exposed with an `_info` suffix on the public name.
        let view = registry
            .view()
            .with_full_name("build_info")
            .with_const_label((Cow::Borrowed("component"), Cow::Borrowed("auth")));

        let mut output = String::new();
        encode(&mut output, &view).unwrap();

        assert_eq!(text_family_names(&output), vec!["build"]);
        assert!(output.contains(r#"build_info{component="auth",version="1.0"} 1"#));

        // A non-matching label condition excludes the family entirely.
        let view = registry
            .view()
            .with_const_label((Cow::Borrowed("component"), Cow::Borrowed("other")));
        let mut output = String::new();
        encode(&mut output, &view).unwrap();
        assert_eq!(output, "# EOF\n");
    }

    #[test]
    fn registry_view_const_label_order_independent() {
        let mut registry = Registry::default();

        // Declared in one order.
        registry
            .sub_registry_with_labels(
                [
                    (Cow::Borrowed("a"), Cow::Borrowed("1")),
                    (Cow::Borrowed("b"), Cow::Borrowed("2")),
                ]
                .into_iter(),
            )
            .register("m1", "", Counter::<u64>::default());

        // Declared in the opposite order.
        registry
            .sub_registry_with_labels(
                [
                    (Cow::Borrowed("b"), Cow::Borrowed("2")),
                    (Cow::Borrowed("a"), Cow::Borrowed("1")),
                ]
                .into_iter(),
            )
            .register("m2", "", Counter::<u64>::default());

        // Different value.
        registry
            .sub_registry_with_labels(
                [
                    (Cow::Borrowed("a"), Cow::Borrowed("9")),
                    (Cow::Borrowed("b"), Cow::Borrowed("2")),
                ]
                .into_iter(),
            )
            .register("m3", "", Counter::<u64>::default());

        // Queried in yet another order, via two stacked conditions.
        let view = registry
            .view()
            .with_const_label((Cow::Borrowed("b"), Cow::Borrowed("2")))
            .with_const_label((Cow::Borrowed("a"), Cow::Borrowed("1")));

        let mut output = String::new();
        encode(&mut output, &view).unwrap();
        assert_eq!(text_family_names(&output), vec!["m1", "m2"]);
        assert!(!output.contains("m3"));
        assert_no_half_descriptors(&output);
    }

    #[test]
    fn registry_view_empty_equals_registry() {
        let mut registry = Registry::default();
        registry.register("a", "", Gauge::<i64>::default());
        registry
            .sub_registry_with_prefix("p")
            .register("b", "", Counter::<u64>::default());

        // A view without any conditions encodes exactly the same output.
        let mut full = String::new();
        encode(&mut full, &registry).unwrap();

        let mut view_output = String::new();
        encode(&mut view_output, &registry.view()).unwrap();

        assert_eq!(full, view_output);

        // A view whose conditions never match produces just the EOF marker
        // without any descriptor.
        let mut empty = String::new();
        encode(
            &mut empty,
            &registry.view().with_name_prefix("does_not_exist"),
        )
        .unwrap();
        assert_eq!(empty, "# EOF\n");

        let mut empty = String::new();
        encode(&mut empty, &registry.view().with_full_name("nope")).unwrap();
        assert_eq!(empty, "# EOF\n");
    }

    #[test]
    fn registry_view_sees_registrations_made_after_creation() {
        let mut registry = Registry::default();
        fn api_view(registry: &Registry) -> RegistryView<'_> {
            registry.view().with_name_prefix("api")
        }

        // Nothing matches yet.
        let mut output = String::new();
        encode(&mut output, &api_view(&registry)).unwrap();
        assert_eq!(output, "# EOF\n");

        // Register after the view was created.
        registry.sub_registry_with_prefix("api").register(
            "requests",
            "",
            Counter::<u64>::default(),
        );
        registry.register("ui_hits", "", Counter::<u64>::default());

        let mut output = String::new();
        encode(&mut output, &api_view(&registry)).unwrap();
        assert_eq!(text_family_names(&output), vec!["api_requests"]);
        assert!(output.contains("api_requests_total"));
        assert!(!output.contains("ui_hits_total"));
    }

    #[test]
    fn registry_view_stable_during_encode() {
        let mut registry = Registry::default();
        registry.register("first", "", Gauge::<i64>::default());

        // Phase one of an exposition: the descriptor set is collected up
        // front. This is the exact boundary after which a concurrent
        // registration must no longer affect the ongoing exposition.
        let filter = ViewFilter {
            prefixes: vec![],
            names: vec![],
            required_labels: vec![],
        };
        let mut entries = Vec::new();
        registry.collect(
            &mut entries,
            registry.prefix.as_ref(),
            &registry.labels,
            Some(&filter),
        );

        // Phase two: emit the previously collected descriptors. Rendering
        // here stands in for the moment the encoder would start writing; in a
        // real exposition this happens before any concurrent registration can
        // take effect for the current scrape.
        let buffer = {
            let mut buffer = String::new();
            let mut descriptor_encoder =
                crate::encoding::text::DescriptorEncoder::new(&mut buffer).into();
            encode_entries(entries, &mut descriptor_encoder).unwrap();
            buffer
        };

        // The descriptor snapshot is fixed: registering after collection does
        // not change the output that was just produced.
        registry.register("registered_mid_encode", "", Gauge::<i64>::default());

        assert_eq!(text_family_names(&buffer), vec!["first"]);
        assert!(!buffer.contains("registered_mid_encode"));

        // The following exposition picks the new family up.
        let mut output = String::new();
        encode(&mut output, &registry.view()).unwrap();
        assert_eq!(
            text_family_names(&output),
            vec!["first", "registered_mid_encode"]
        );
    }

    #[cfg(any(feature = "prometheus_protobuf", feature = "openmetrics_protobuf"))]
    #[test]
    fn registry_view_text_and_protobuf_select_same_families() {
        let mut registry = Registry::default();
        registry.register("keep_gauge", "", Gauge::<i64>::default());
        registry.register("skip_gauge", "", Gauge::<i64>::default());

        let sub = registry.sub_registry_with_prefix("api");
        sub.register("requests", "", Counter::<u64>::default());
        sub.register("dropped", "", Counter::<u64>::default());

        sub.sub_registry_with_label((Cow::Borrowed("tenant"), Cow::Borrowed("x")))
            .register("tenanted", "", Gauge::<i64>::default());

        let apiary: Gauge = Gauge::<i64>::default();
        registry.register("apiary_count", "", apiary);

        let view = registry.view().with_name_prefix("api");

        let mut text_output = String::new();
        encode(&mut text_output, &view).unwrap();

        let text_names = text_family_names(&text_output);
        println!("text selected families: {text_names:?}");

        // The prefix boundary excludes `apiary_count`; no unit families are
        // present, hence text TYPE names and public family names coincide for
        // gauges. Counter TYPE lines lack `_total`, normalize for comparison.
        assert_eq!(
            text_names,
            vec!["api_requests", "api_dropped", "api_tenanted"]
        );
        assert!(!text_names.contains(&"apiary_count"));
        assert_no_half_descriptors(&text_output);

        #[cfg(feature = "prometheus_protobuf")]
        {
            let families = crate::encoding::prometheus_protobuf::encode(&view).unwrap();
            let proto_names: Vec<&str> = families.iter().map(|f| f.name.as_str()).collect();
            println!("prometheus protobuf selected families: {proto_names:?}");

            assert_eq!(
                proto_names,
                vec!["api_requests_total", "api_dropped_total", "api_tenanted",]
            );

            // Same set and order after normalizing the counter suffix.
            let normalized: Vec<&str> = proto_names
                .iter()
                .map(|name| name.strip_suffix("_total").unwrap_or(name))
                .collect();
            assert_eq!(normalized, text_names);

            // Tenanted proto family keeps the expanded const label.
            let tenanted = families.iter().find(|f| f.name == "api_tenanted").unwrap();
            assert_eq!(tenanted.metric.len(), 1);
            assert!(tenanted.metric[0]
                .label
                .iter()
                .any(|l| l.name == "tenant" && l.value == "x"));
        }

        #[cfg(feature = "openmetrics_protobuf")]
        {
            let metric_set = crate::encoding::openmetrics_protobuf::encode(&view).unwrap();
            let proto_names: Vec<&str> = metric_set
                .metric_families
                .iter()
                .map(|f| f.name.as_str())
                .collect();
            println!("openmetrics protobuf selected families: {proto_names:?}");
            assert_eq!(
                proto_names,
                vec!["api_requests", "api_dropped", "api_tenanted"]
            );
            assert_eq!(proto_names, text_names);
        }
    }
}
