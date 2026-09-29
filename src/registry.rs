//! Metric registry implementation.
//!
//! See [`Registry`] for details.

use std::borrow::Cow;

use crate::collector::Collector;
use crate::encoding::{DescriptorEncoder, EncodeMetric};

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

    /// Create a [`RegistryView`] over this [`Registry`].
    ///
    /// The view is a live, read-only view: it does not copy any metric
    /// objects and does not change the registration order of the registry.
    /// Metrics registered with the underlying registry after the view was
    /// created are picked up by later encodings.
    ///
    /// Without any additional conditions the view exposes the exact same
    /// metric families as the registry itself. Conditions are composed via
    /// intersection, i.e. a family has to satisfy every condition that was
    /// added to the view.
    ///
    /// A view can be encoded with the same encoders as a registry, e.g.
    /// [`crate::encoding::text::encode`] or
    /// [`crate::encoding::prometheus_protobuf::encode`].
    ///
    /// ```
    /// # use prometheus_client::encoding::text::encode;
    /// # use prometheus_client::metrics::counter::{Atomic as _, Counter};
    /// # use prometheus_client::registry::Registry;
    /// #
    /// let mut registry = Registry::default();
    ///
    /// let api_counter: Counter = Counter::<u64>::default();
    /// registry.register("api_requests", "API requests", api_counter.clone());
    /// let other_counter: Counter = Counter::<u64>::default();
    /// registry.register("worker_jobs", "Worker jobs", other_counter.clone());
    ///
    /// // Only expose metric families whose name starts with `api_`.
    /// let view = registry.view().with_prefix("api_");
    ///
    /// let mut buffer = String::new();
    /// encode(&mut buffer, &view).unwrap();
    /// assert!(buffer.contains("api_requests"));
    /// assert!(!buffer.contains("worker_jobs"));
    /// ```
    pub fn view(&self) -> RegistryView<'_> {
        RegistryView {
            registry: self,
            condition: ViewCondition::default(),
        }
    }

    /// Create a [`RegistryView`] from a previously built
    /// [`ViewSelector`].
    ///
    /// This is useful when a registry is shared behind a lock: the selector
    /// can be kept around while the lock guard is released and re-acquired
    /// for each scrape.
    ///
    /// ```
    /// # use prometheus_client::metrics::counter::Counter;
    /// # use prometheus_client::registry::{Registry, ViewSelector};
    /// #
    /// let mut registry = Registry::default();
    /// registry.register("api_requests", "", Counter::<u64>::default());
    ///
    /// let selector = registry.view().with_prefix("api").into_selector();
    /// let view = registry.view_with(&selector);
    /// let _ = view;
    /// ```
    pub fn view_with<'a>(&'a self, selector: &'a ViewSelector) -> RegistryView<'a> {
        RegistryView {
            registry: self,
            condition: selector.0.clone(),
        }
    }

    pub(crate) fn encode(&self, encoder: &mut DescriptorEncoder) -> Result<(), std::fmt::Error> {
        self.encode_viewed(encoder, &ViewCondition::default())
    }

    pub(crate) fn encode_viewed(
        &self,
        encoder: &mut DescriptorEncoder,
        condition: &ViewCondition,
    ) -> Result<(), std::fmt::Error> {
        // Snapshot the set of descriptors visible under the given condition up
        // front so that registrations happening while encoding is in progress
        // can not result in a partially emitted exposition. Newly registered
        // metrics are picked up by the next encoding.
        let mut selection: Vec<SelectedEntry<'_>> = Vec::new();
        self.collect_entries(condition, &mut selection);

        for entry in selection {
            match entry {
                SelectedEntry::Metric {
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
                SelectedEntry::Collector {
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

    /// Collect all currently visible entries in registration order.
    fn collect_entries<'a>(
        &'a self,
        condition: &ViewCondition,
        selection: &mut Vec<SelectedEntry<'a>>,
    ) {
        for (descriptor, metric) in self.metrics.iter() {
            if metric.is_empty() {
                continue;
            }
            if condition.matches(self.prefix.as_ref(), &self.labels, descriptor) {
                selection.push(SelectedEntry::Metric {
                    prefix: self.prefix.as_ref(),
                    labels: &self.labels,
                    descriptor,
                    metric: metric.as_ref(),
                });
            }
        }

        for collector in self.collectors.iter() {
            if condition.matches_collector(self.prefix.as_ref(), &self.labels) {
                selection.push(SelectedEntry::Collector {
                    prefix: self.prefix.as_ref(),
                    labels: &self.labels,
                    collector: collector.as_ref(),
                });
            }
        }

        for registry in self.sub_registries.iter() {
            registry.collect_entries(condition, selection);
        }
    }
}

/// A live, read-only, composable view of a [`Registry`].
///
/// A `RegistryView` shares the registered metric objects with its registry;
/// it neither copies metrics nor alters the registry's registration order.
/// Conditions added to a view are combined by intersection, i.e. a metric
/// family is only exposed when it satisfies all of them. A view without
/// conditions is equivalent to encoding the registry itself.
///
/// The view is live: metric families registered with the underlying registry
/// after the view was created are visible in later encodings. Each individual
/// encoding uses a stable snapshot of the descriptor set, so families
/// registered while an encoding is in progress only appear in the next
/// encoding instead of resulting in a half-emitted exposition.
///
/// Both the OpenMetrics text format and the protobuf formats apply the same
/// selection and order, so a view can be passed to
/// [`crate::encoding::text::encode`],
/// [`crate::encoding::prometheus_protobuf::encode`] and
/// [`crate::encoding::openmetrics_protobuf::encode`] alike.
#[derive(Debug)]
pub struct RegistryView<'a> {
    registry: &'a Registry,
    condition: ViewCondition,
}

impl<'a> RegistryView<'a> {
    /// Restrict the view to metric families whose final public name starts
    /// with `prefix`.
    ///
    /// The boundary is aligned to Prometheus metric name components: both
    /// `api` and `api_` match `api_requests` but neither matches
    /// `apiary_jobs`, i.e. the character following the prefix must either
    /// end the name or be `_` (a trailing `_` in the prefix is treated as the
    /// separator).
    ///
    /// Calling this method multiple times further restricts the view; a family
    /// has to start with every prefix added.
    ///
    /// ```
    /// # use prometheus_client::encoding::text::encode;
    /// # use prometheus_client::metrics::counter::Counter;
    /// # use prometheus_client::registry::Registry;
    /// #
    /// let mut registry = Registry::default();
    /// registry.register("api_requests", "", Counter::<u64>::default());
    /// registry.register("apiary_jobs", "", Counter::<u64>::default());
    ///
    /// let view = registry.view().with_prefix("api");
    ///
    /// let mut buffer = String::new();
    /// encode(&mut buffer, &view).unwrap();
    /// assert!(buffer.contains("api_requests"));
    /// assert!(!buffer.contains("apiary_jobs"));
    /// ```
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.condition.add_prefix(prefix.into());
        self
    }

    /// Restrict the view to the metric family with the given fully expanded
    /// family name.
    ///
    /// The name is matched against the OpenMetrics metric family name,
    /// including sub-registry prefixes and an optional unit, e.g. a counter
    /// registered as `http_requests` with unit `seconds` is matched by
    /// `http_requests_seconds`. Type-specific sample suffixes such as the
    /// counter's `_total` and the info metric's `_info` are not part of the
    /// family name and must not be provided; they are still emitted in the
    /// sample output (e.g. the counter above exposes `http_requests_seconds_total`
    /// samples) and in the Prometheus protobuf family name.
    ///
    /// Calling this method multiple times further restricts the view; the
    /// families have to match every name added.
    ///
    /// ```
    /// # use prometheus_client::encoding::text::encode;
    /// # use prometheus_client::metrics::counter::Counter;
    /// # use prometheus_client::registry::Registry;
    /// #
    /// let mut registry = Registry::default();
    /// registry.register("foo", "", Counter::<u64>::default());
    /// registry.register("bar", "", Counter::<u64>::default());
    ///
    /// let view = registry.view().with_name("foo");
    ///
    /// let mut buffer = String::new();
    /// encode(&mut buffer, &view).unwrap();
    /// assert!(buffer.contains("foo_total"));
    /// assert!(!buffer.contains("bar_total"));
    /// ```
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.condition.add_name(name.into());
        self
    }

    /// Restrict the view to metric families carrying a const label with the
    /// given encoded key and value.
    ///
    /// Const labels are the labels introduced via
    /// [`Registry::with_labels`], [`Registry::sub_registry_with_label`] and
    /// [`Registry::sub_registry_with_labels`], with sub-registry prefixes and
    /// labels expanded first. Keys and values are compared in their fully
    /// encoded form and are not affected by the order in which labels were
    /// declared.
    ///
    /// Calling this method multiple times further restricts the view; the
    /// families have to carry every label added.
    ///
    /// ```
    /// # use prometheus_client::encoding::text::encode;
    /// # use prometheus_client::metrics::counter::Counter;
    /// # use prometheus_client::registry::Registry;
    /// # use std::borrow::Cow;
    /// #
    /// let mut registry = Registry::default();
    /// let tenant_a = registry.sub_registry_with_label((
    ///     Cow::Borrowed("tenant"),
    ///     Cow::Borrowed("a"),
    /// ));
    /// tenant_a.register("requests", "", Counter::<u64>::default());
    /// let tenant_b = registry.sub_registry_with_label((
    ///     Cow::Borrowed("tenant"),
    ///     Cow::Borrowed("b"),
    /// ));
    /// tenant_b.register("requests", "", Counter::<u64>::default());
    ///
    /// let view = registry.view().with_label("tenant", "a");
    ///
    /// let mut buffer = String::new();
    /// encode(&mut buffer, &view).unwrap();
    /// assert!(buffer.contains(r#"tenant="a""#));
    /// assert!(!buffer.contains(r#"tenant="b""#));
    /// ```
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.condition.add_label(key.into(), value.into());
        self
    }

    /// Turn this view into an owned [`ViewSelector`] carrying only the
    /// view's conditions, without borrowing the registry.
    ///
    /// Pair this with [`Registry::view_with`] to rebuild the view against
    /// the same registry later, e.g. after releasing a lock guard.
    pub fn into_selector(self) -> ViewSelector {
        ViewSelector(self.condition)
    }

    pub(crate) fn encode(&self, encoder: &mut DescriptorEncoder) -> Result<(), std::fmt::Error> {
        self.registry.encode_viewed(encoder, &self.condition)
    }
}

/// Owned, reusable set of conditions extracted from a [`RegistryView`].
///
/// A `ViewSelector` does not borrow the registry. Rebuild the actual view
/// whenever needed with [`Registry::view_with`]. Conditions of different
/// selectors are still combined by intersection within the view they came
/// from.
///
/// ```
/// # use prometheus_client::encoding::text::encode;
/// # use prometheus_client::metrics::counter::Counter;
/// # use prometheus_client::registry::{Registry, ViewSelector};
/// # use std::sync::{Arc, Mutex};
/// #
/// let registry = Arc::new(Mutex::new(Registry::default()));
///
/// // Build the reusable conditions once, without holding the lock.
/// let selector = ViewSelector::new().with_prefix("api");
///
/// {
///     let mut registry = registry.lock().unwrap();
///     registry.register("api_requests", "", Counter::<u64>::default());
/// }
///
/// // Rebuild the view on each scrape after acquiring the lock.
/// let registry = registry.lock().unwrap();
/// let view = registry.view_with(&selector);
/// let mut buffer = String::new();
/// encode(&mut buffer, &view).unwrap();
/// assert!(buffer.contains("api_requests_total"));
/// ```
#[derive(Debug, Clone, Default)]
pub struct ViewSelector(ViewCondition);

impl ViewSelector {
    /// Create an empty selector, i.e. one without any conditions.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a name prefix condition. See [`RegistryView::with_prefix`].
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.0.add_prefix(prefix.into());
        self
    }

    /// Add a full metric name condition. See [`RegistryView::with_name`].
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.0.add_name(name.into());
        self
    }

    /// Add a const label condition. See [`RegistryView::with_label`].
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.0.add_label(key.into(), value.into());
        self
    }
}

/// Conditions of a [`RegistryView`], combined by intersection.
#[derive(Debug, Default, Clone)]
pub(crate) struct ViewCondition {
    prefixes: Vec<String>,
    names: Vec<String>,
    labels: Vec<(String, String)>,
}

impl ViewCondition {
    fn add_prefix(&mut self, prefix: String) {
        self.prefixes.push(prefix);
    }

    fn add_name(&mut self, name: String) {
        self.names.push(name);
    }

    fn add_label(&mut self, key: String, value: String) {
        self.labels.push((key, value));
    }

    /// Whether a metric registered in a registry with the given prefix and
    /// labels is exposed by this condition.
    fn matches(
        &self,
        registry_prefix: Option<&Prefix>,
        registry_labels: &[(Cow<'static, str>, Cow<'static, str>)],
        descriptor: &Descriptor,
    ) -> bool {
        let name = full_metric_name(registry_prefix, &descriptor.name, descriptor.unit.as_ref());

        self.matches_name(&name) && self.matches_labels(registry_labels)
    }

    /// Whether a collector registered in a registry with the given prefix and
    /// labels is exposed by this condition.
    ///
    /// Collectors emit their own descriptor names on each scrape which can not
    /// be known ahead of time, hence name conditions can not be evaluated for
    /// them. They are only included when no name condition is present; label
    /// conditions still apply.
    fn matches_collector(
        &self,
        _registry_prefix: Option<&Prefix>,
        registry_labels: &[(Cow<'static, str>, Cow<'static, str>)],
    ) -> bool {
        self.prefixes.is_empty() && self.names.is_empty() && self.matches_labels(registry_labels)
    }

    fn matches_name(&self, name: &str) -> bool {
        self.prefixes
            .iter()
            .all(|prefix| prefix_matches(name, prefix))
            && self.names.iter().all(|expected| name == expected.as_str())
    }

    fn matches_labels(&self, registry_labels: &[(Cow<'static, str>, Cow<'static, str>)]) -> bool {
        self.labels.iter().all(|(key, value)| {
            registry_labels
                .iter()
                .any(|(candidate_key, candidate_value)| {
                    candidate_key.as_ref() == key.as_str()
                        && candidate_value.as_ref() == value.as_str()
                })
        })
    }
}

/// Check that `prefix` ends on a metric name component boundary in `name`.
///
/// A trailing `_` in the prefix is a separator rather than part of the name
/// component, i.e. both `api` and `api_` match `api_requests` while neither
/// matches `apiary_jobs`.
fn prefix_matches(name: &str, prefix: &str) -> bool {
    let prefix = prefix.strip_suffix('_').unwrap_or(prefix);

    let Some(rest) = name.strip_prefix(prefix) else {
        return false;
    };

    match rest.as_bytes().first() {
        None => true,
        Some(next) => *next == b'_',
    }
}

/// Build the final public metric name of a registered metric, matching the
/// family name used by the encoders (`prefix_name_unit`).
///
/// Type-specific sample suffixes such as a counter's `_total` or an info
/// metric's `_info` are intentionally not included: the OpenMetrics family
/// name (used for `# HELP`, `# TYPE`, `# UNIT` and the OpenMetrics protobuf
/// family name) is the base name, and the Prometheus protobuf encoder derives
/// the suffixed sample family name from the same base. Filtering on the base
/// name therefore yields the same family set on all encoders.
fn full_metric_name(prefix: Option<&Prefix>, name: &str, unit: Option<&Unit>) -> String {
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

    full_name
}

/// One entry selected for a single encoding, in registration order.
enum SelectedEntry<'a> {
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

/// Internal dispatch type shared by the encoders so that both a whole
/// [`Registry`] and a [`RegistryView`] can be encoded.
///
/// Hidden from documentation; it only exists to share the encoder entry
/// points.
#[doc(hidden)]
#[derive(Debug)]
pub enum RegistryRef<'a> {
    Registry(&'a Registry),
    View(&'a RegistryView<'a>),
}

impl RegistryRef<'_> {
    #[doc(hidden)]
    pub fn encode(&self, encoder: &mut DescriptorEncoder) -> Result<(), std::fmt::Error> {
        match self {
            RegistryRef::Registry(registry) => registry.encode(encoder),
            RegistryRef::View(view) => view.encode(encoder),
        }
    }
}

/// Conversion into a [`RegistryRef`], allowing encoders to accept a
/// [`Registry`], a [`RegistryView`], a reference to either, or an
/// [`std::sync::Arc`] wrapping one.
///
/// This trait is sealed and only implemented for types provided by this
/// crate. It is hidden from the documentation as it only exists to share the
/// different encoder entry points.
#[doc(hidden)]
pub trait AsRegistryRef: sealed::Sealed {
    fn as_registry_ref(&self) -> RegistryRef<'_>;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Registry {}
    impl Sealed for super::RegistryView<'_> {}
    impl<T: super::AsRegistryRef + ?Sized> Sealed for &T {}
    impl<T: super::AsRegistryRef + ?Sized> Sealed for std::sync::Arc<T> {}
}

impl AsRegistryRef for Registry {
    fn as_registry_ref(&self) -> RegistryRef<'_> {
        RegistryRef::Registry(self)
    }
}

impl AsRegistryRef for RegistryView<'_> {
    fn as_registry_ref(&self) -> RegistryRef<'_> {
        RegistryRef::View(self)
    }
}

impl<T: AsRegistryRef + ?Sized> AsRegistryRef for &T {
    fn as_registry_ref(&self) -> RegistryRef<'_> {
        (*self).as_registry_ref()
    }
}

impl<T: AsRegistryRef + ?Sized> AsRegistryRef for std::sync::Arc<T> {
    fn as_registry_ref(&self) -> RegistryRef<'_> {
        self.as_ref().as_registry_ref()
    }
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
mod registry_view_tests {
    use super::*;
    use crate::encoding::text;
    use crate::encoding::{EncodeMetric, MetricEncoder};
    use crate::metrics::counter::Counter;
    use crate::metrics::gauge::Gauge;
    use crate::metrics::info::Info;
    use crate::metrics::MetricType;
    use std::borrow::Cow;
    use std::sync::{Arc, Condvar, Mutex};

    #[cfg(feature = "openmetrics_protobuf")]
    use crate::encoding::openmetrics_protobuf;
    #[cfg(feature = "prometheus_protobuf")]
    use crate::encoding::prometheus_protobuf;

    #[derive(Debug)]
    struct EncodeBarrier {
        entered: bool,
    }

    #[derive(Debug)]
    struct BarrierMetric {
        state: Arc<(Mutex<EncodeBarrier>, Condvar)>,
        counter: Counter,
    }

    impl BarrierMetric {
        fn new(state: Arc<(Mutex<EncodeBarrier>, Condvar)>) -> Self {
            Self {
                state,
                counter: Counter::<u64>::default(),
            }
        }
    }

    impl EncodeMetric for BarrierMetric {
        fn encode(&self, encoder: MetricEncoder) -> Result<(), std::fmt::Error> {
            let (lock, cvar) = self.state.as_ref();
            let mut state = lock.lock().unwrap();
            state.entered = true;
            cvar.notify_all();
            // Give a concurrent registration a chance to run while this
            // encoding is in progress.
            let (guard, _timeout) = cvar
                .wait_timeout(state, std::time::Duration::from_millis(100))
                .unwrap();
            drop(guard);
            self.counter.encode(encoder)
        }

        fn metric_type(&self) -> MetricType {
            MetricType::Counter
        }
    }

    fn build_nested_registry() -> Registry {
        let mut registry = Registry::default();

        registry.register("top_gauge", "Top level gauge", Gauge::<i64>::default());

        let first = registry.sub_registry_with_prefix("api");
        first.register("requests", "API requests", Counter::<u64>::default());

        let second = first
            .sub_registry_with_label((Cow::Borrowed("tenant"), Cow::Borrowed("a")))
            .sub_registry_with_prefix("v2")
            .sub_registry_with_label((Cow::Borrowed("env"), Cow::Borrowed("prod")));
        second.register("errors", "Nested errors", Counter::<u64>::default());

        let other = registry.sub_registry_with_prefix("apiary");
        other.register(
            "jobs",
            "Should not match the api prefix",
            Counter::<u64>::default(),
        );

        registry
    }

    #[cfg(any(feature = "prometheus_protobuf", feature = "openmetrics_protobuf"))]
    fn text_family_names(output: &str) -> Vec<String> {
        output
            .lines()
            .filter_map(|line| line.strip_prefix("# TYPE "))
            .map(|line| line.split(' ').next().unwrap().to_string())
            .collect()
    }

    #[test]
    fn nested_sub_registries_are_expanded_before_filtering() {
        let registry = build_nested_registry();

        let view = registry.view().with_label("tenant", "a");
        let mut buffer = String::new();
        text::encode(&mut buffer, &view).unwrap();

        assert!(buffer.contains("api_v2_errors_total{tenant=\"a\",env=\"prod\"}"));
        assert!(!buffer.contains("top_gauge"));
        assert!(!buffer.contains("api_requests"));
        assert!(!buffer.contains("apiary_jobs"));
    }

    #[test]
    fn prefix_respects_metric_name_boundary() {
        let registry = build_nested_registry();

        let view = registry.view().with_prefix("api");
        let mut buffer = String::new();
        text::encode(&mut buffer, &view).unwrap();

        assert!(buffer.contains("api_requests_total"));
        assert!(buffer.contains("api_v2_errors_total"));
        assert!(!buffer.contains("apiary_jobs_total"));
        assert!(!buffer.contains("top_gauge"));
    }

    #[test]
    fn full_family_name_matches_including_unit_but_not_sample_suffix() {
        let mut registry = Registry::default();
        registry.register_with_unit(
            "requests",
            "Requests",
            Unit::Seconds,
            Counter::<u64>::default(),
        );
        registry.register("requests", "Requests gauge", Gauge::<i64>::default());

        let view = registry.view().with_name("requests_seconds");
        let mut buffer = String::new();
        text::encode(&mut buffer, &view).unwrap();

        assert!(buffer.contains("requests_seconds_total"));
        assert!(!buffer.contains("# TYPE requests gauge\n"));
    }

    #[test]
    fn info_family_name_does_not_carry_sample_suffix() {
        let mut registry = Registry::default();
        let info: Info<Vec<(String, String)>> =
            Info::new(vec![("version".to_string(), "1".to_string())]);
        registry.register("build", "Build info", info);

        let view = registry.view().with_name("build");
        let mut buffer = String::new();
        text::encode(&mut buffer, &view).unwrap();

        assert!(buffer.contains("build_info"));
    }

    #[test]
    fn label_order_does_not_affect_filtering() {
        let mut registry = Registry::default();

        let one = registry.sub_registry_with_labels(
            vec![
                (Cow::Borrowed("tenant"), Cow::Borrowed("a")),
                (Cow::Borrowed("env"), Cow::Borrowed("prod")),
            ]
            .into_iter(),
        );
        one.register("requests", "", Counter::<u64>::default());

        let two = registry.sub_registry_with_labels(
            vec![
                (Cow::Borrowed("env"), Cow::Borrowed("prod")),
                (Cow::Borrowed("tenant"), Cow::Borrowed("a")),
            ]
            .into_iter(),
        );
        two.register("jobs", "", Counter::<u64>::default());

        let view = registry
            .view()
            .with_label("tenant", "a")
            .with_label("env", "prod");

        let mut buffer = String::new();
        text::encode(&mut buffer, &view).unwrap();

        assert!(buffer.contains("requests_total{tenant=\"a\",env=\"prod\"}"));
        assert!(buffer.contains("jobs_total{env=\"prod\",tenant=\"a\"}"));
    }

    #[test]
    fn conditions_combine_by_intersection() {
        let registry = build_nested_registry();

        let view = registry.view().with_prefix("api").with_label("tenant", "a");

        let mut buffer = String::new();
        text::encode(&mut buffer, &view).unwrap();

        assert!(buffer.contains("api_v2_errors_total"));
        assert!(!buffer.contains("api_requests_total"));
        assert!(!buffer.contains("apiary_jobs_total"));
    }

    #[test]
    fn empty_condition_view_is_equivalent_to_registry() {
        let registry = build_nested_registry();

        let mut registry_output = String::new();
        text::encode(&mut registry_output, &registry).unwrap();

        let mut view_output = String::new();
        text::encode(&mut view_output, &registry.view()).unwrap();

        assert_eq!(registry_output, view_output);
    }

    #[test]
    fn view_without_any_match_emits_only_eof() {
        let registry = build_nested_registry();

        let view = registry.view().with_name("does_not_exist");
        let mut buffer = String::new();
        text::encode(&mut buffer, &view).unwrap();

        assert_eq!(buffer, "# EOF\n");
    }

    #[test]
    fn view_sees_metrics_registered_after_creation() {
        let registry = Arc::new(Mutex::new(Registry::default()));
        let selector;
        {
            let mut guard = registry.lock().unwrap();
            guard.register("early", "", Counter::<u64>::default());
        }
        {
            let guard = registry.lock().unwrap();
            let view = guard.view().with_prefix("late");

            let mut buffer = String::new();
            text::encode(&mut buffer, &view).unwrap();
            assert_eq!(buffer, "# EOF\n");

            selector = view.into_selector();
        }

        {
            let mut guard = registry.lock().unwrap();
            guard.register("late_metric", "", Gauge::<i64>::default());
        }

        let guard = registry.lock().unwrap();
        let view = guard.view_with(&selector);
        let mut buffer = String::new();
        text::encode(&mut buffer, &view).unwrap();
        assert!(buffer.contains("late_metric"));
    }

    #[test]
    fn registration_during_encoding_waits_for_next_scrape() {
        let registry: Arc<Mutex<Registry>> = Arc::new(Mutex::new(Registry::default()));
        let barrier = Arc::new((Mutex::new(EncodeBarrier { entered: false }), Condvar::new()));

        {
            let mut guard = registry.lock().unwrap();
            guard.register("trigger", "", BarrierMetric::new(Arc::clone(&barrier)));
        }

        let encoder_registry = Arc::clone(&registry);
        let encoder = std::thread::spawn(move || {
            let guard = encoder_registry.lock().unwrap();
            let view = guard.view();
            let mut buffer = String::new();
            text::encode(&mut buffer, &view).unwrap();
            buffer
        });

        // Wait until the encoder is inside metric encoding, then register a
        // new family while the first scrape is still in progress.
        let (lock, cvar) = barrier.as_ref();
        {
            let mut state = lock.lock().unwrap();
            while !state.entered {
                state = cvar.wait(state).unwrap();
            }
        }
        {
            let mut guard = registry.lock().unwrap();
            guard.register(
                "late_metric",
                "Registered during encoding",
                Gauge::<i64>::default(),
            );
        }

        let first = encoder.join().unwrap();
        assert!(first.contains("trigger_total"));
        assert!(!first.contains("late_metric"));
        assert!(first.ends_with("# EOF\n"));

        let guard = registry.lock().unwrap();
        let mut second = String::new();
        text::encode(&mut second, &guard.view()).unwrap();
        assert!(second.contains("trigger_total"));
        assert!(second.contains("# HELP late_metric Registered during encoding."));
        assert!(second.ends_with("# EOF\n"));
    }

    #[cfg(feature = "prometheus_protobuf")]
    #[test]
    fn text_and_protobuf_select_the_same_families_in_order() {
        let registry = build_nested_registry();
        let view = registry.view().with_prefix("api");

        let mut text_output = String::new();
        text::encode(&mut text_output, &view).unwrap();
        let text_names = text_family_names(&text_output);

        let proto_families = prometheus_protobuf::encode(&view).unwrap();
        let proto_names: Vec<String> = proto_families
            .iter()
            .map(|family| family.name.clone())
            .collect();

        // The text format uses the family base name in the `# TYPE` line
        // whereas the Prometheus protobuf family name carries the type
        // suffix (`_total`/`_info`). Normalize both to the family base name to
        // assert that the same families were selected, in the same order.
        let normalized_proto_names: Vec<String> = proto_names
            .iter()
            .map(|name| {
                name.strip_suffix("_total")
                    .or_else(|| name.strip_suffix("_info"))
                    .unwrap_or(name.as_str())
                    .to_string()
            })
            .collect();

        println!("text format selected families:     {:?}", text_names);
        println!(
            "protobuf format selected families:   {:?}",
            normalized_proto_names
        );

        assert_eq!(text_names, normalized_proto_names);
        assert_eq!(
            normalized_proto_names,
            vec!["api_requests".to_string(), "api_v2_errors".to_string()]
        );
        assert_eq!(
            proto_names,
            vec![
                "api_requests_total".to_string(),
                "api_v2_errors_total".to_string()
            ]
        );

        // The sample lines in the text output carry the same public names as
        // the protobuf family names.
        for proto_name in &proto_names {
            assert!(
                text_output.contains(proto_name.as_str()),
                "text output is missing sample family {proto_name}"
            );
        }
    }

    #[cfg(feature = "openmetrics_protobuf")]
    #[test]
    fn text_and_openmetrics_protobuf_select_the_same_families_in_order() {
        let registry = build_nested_registry();
        let view = registry.view().with_prefix("api");

        let mut text_output = String::new();
        text::encode(&mut text_output, &view).unwrap();
        let text_names = text_family_names(&text_output);

        let metric_set = openmetrics_protobuf::encode(&view).unwrap();
        let proto_names: Vec<String> = metric_set
            .metric_families
            .iter()
            .map(|family| family.name.clone())
            .collect();

        assert_eq!(text_names, proto_names);
        assert_eq!(
            proto_names,
            vec!["api_requests".to_string(), "api_v2_errors".to_string()]
        );
    }

    #[cfg(feature = "prometheus_protobuf")]
    #[test]
    fn text_and_protobuf_agree_on_empty_view() {
        let registry = build_nested_registry();
        let view = registry.view().with_name("does_not_exist");

        let mut text_output = String::new();
        text::encode(&mut text_output, &view).unwrap();
        assert_eq!(text_output, "# EOF\n");

        let proto_families = prometheus_protobuf::encode(&view).unwrap();
        assert!(proto_families.is_empty());
    }
}
