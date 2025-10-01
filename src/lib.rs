use core::ops::{ Deref, DerefMut };
use bevy_ecs::{
    component::{
        ComponentId,
        Tick
    },
    resource::Resource,
    system::{
        IntoSystem,
        SystemInput,
        SystemParam,
        System,
        SystemState,
        SystemMeta,
        PipeSystem,
        CombinatorSystem,
        Combine,
        AdapterSystem,
        Adapt,
        FunctionSystem,
        SystemParamFunction,
        ResMut
    },
    world::{
        World,
        unsafe_world_cell::UnsafeWorldCell
    }
};


pub mod prelude {
    pub use crate::{
        AppExt as _,
        Request,
        Callback
    };
}


#[derive(Resource)]
pub struct ErasedCallbackSystem<Event, Out> {
    system : Box<dyn System<In = Request<Event>, Out = Out> + Send + Sync>,
    state  : Box<dyn ErasedSystemParam + Send + Sync>
}

trait ErasedSystemParam {
    fn meta(&self) -> &SystemMeta;
}
impl<Param> ErasedSystemParam for SystemState<Param>
where
    Param : SystemParam
{
    fn meta(&self) -> &SystemMeta {
        SystemState::meta(self)
    }
}


#[cfg(feature = "app")]
pub trait AppExt {
    fn add_callback<Event, Out, S, M>(&mut self, system : S) -> &mut Self
    where
        Event : 'static,
        Out   : 'static,
        S     : IntoCallbackSystem<Event, Out, M>;
}
#[cfg(feature = "app")]
impl AppExt for bevy_app::App {
    fn add_callback<Event, Out, S, M>(&mut self, system : S) -> &mut Self
    where
        Event : 'static,
        Out   : 'static,
        S     : IntoCallbackSystem<Event, Out, M>
    {
        let     world  = self.world_mut();
        let mut system = IntoCallbackSystem::into_system(system);
        let     state  = SystemState::<<S::System as ParametisedSystem>::Param>::new(world);

        let world = self.world_mut();
        if (world.contains_resource::<ErasedCallbackSystem<Event, Out>>()) {
            panic!(
                "Duplicate ({}) -> {} callback registered",
                core::any::type_name::<Event>(),
                core::any::type_name::<Out>()
            );
        }
        system.initialize(world);
        world.insert_resource(ErasedCallbackSystem {
            system : Box::new(system),
            state  : Box::new(state),
        });

        self
    }
}


#[derive(Debug)]
pub struct Request<E>(pub E);

impl<E> Deref for Request<E> {
    type Target = E;
    #[inline]
    fn deref(&self) -> &Self::Target { &self.0 }
}
impl<E> DerefMut for Request<E> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.0 }
}

impl<E> SystemInput for Request<E>
where
    E : 'static
{
    type Param<'i> = Request<E>;
    type Inner<'i> = Request<E>;
    #[inline]
    fn wrap(this: Self::Inner<'_>) -> Self::Param<'_> { this }
}


pub trait ParametisedSystem
where
    Self : System
{
    type Param : SystemParam;
}

impl<A, B> ParametisedSystem for PipeSystem<A, B>
where
            A     : ParametisedSystem,
            B     : ParametisedSystem,
    for<'l> B::In : SystemInput<Inner<'l> = A::Out>
{ type Param = (A::Param, B::Param,); }

impl<A, B, F> ParametisedSystem for CombinatorSystem<F, A, B>
where
    F : Combine<A, B> + 'static,
    A : ParametisedSystem,
    B : ParametisedSystem
{ type Param = (A::Param, B::Param,); }

impl<F, S> ParametisedSystem for AdapterSystem<F, S>
where
    F : Adapt<S>,
    S : ParametisedSystem
{ type Param = S::Param; }

impl<M, F> ParametisedSystem for FunctionSystem<M, F>
where
    M : 'static,
    F : SystemParamFunction<M>
{ type Param = F::Param; }


pub trait CallbackSystem<Event, Out>
where
    Self : ParametisedSystem + System<In = Request<Event>>
{ }

impl<Event, A, B> CallbackSystem<Event, B::Out> for PipeSystem<A, B>
where
            A     : ParametisedSystem<In = Request<Event>>,
            B     : ParametisedSystem,
    for<'l> B::In : SystemInput<Inner<'l> = A::Out>
{ }

impl<Event, A, B, F> CallbackSystem<Event, F::Out> for CombinatorSystem<F, A, B>
where
    F : Combine<A, B, In = Request<Event>> + 'static,
    A : ParametisedSystem,
    B : ParametisedSystem
{ }

impl<Event, F, S> CallbackSystem<Event, F::Out> for AdapterSystem<F, S>
where
    F : Adapt<S, In = Request<Event>>,
    S : ParametisedSystem
{ }

impl<Event, M, F> CallbackSystem<Event, F::Out> for FunctionSystem<M, F>
where
    M : 'static,
    F : SystemParamFunction<M, In = Request<Event>>
{
}


pub trait IntoCallbackSystem<Event, Out, M> {
    type System : CallbackSystem<Event, Out>;
    fn into_system(this : Self) -> Self::System;
}

impl<Event, Out, S, M> IntoCallbackSystem<Event, Out, M> for S
where
    S         : IntoSystem<Request<Event>, Out, M>,
    S::System : CallbackSystem<Event, Out>,
    Event     : 'static
{
    type System = S::System;
    #[inline]
    fn into_system(this : Self) -> Self::System { S::into_system(this) }
}


pub struct Callback<'w, Event, Out>
where
    Event : 'static,
    Out   : 'static
{
    erased : ResMut<'w, ErasedCallbackSystem<Event, Out>>,
    world  : UnsafeWorldCell<'w>
}

impl<Event, Out> Callback<'_, Event, Out> {
    pub fn request(&mut self, event : Event) -> Out {
        unsafe { self.erased.system.run_unsafe(Request(event), self.world) }
    }
}

unsafe impl<Event, Out> SystemParam for Callback<'_, Event, Out> {
    type State        = ComponentId;
    type Item<'w, 's> = Callback<'w, Event, Out>;

    fn init_state(
        world       : &mut World,
        system_meta : &mut SystemMeta
    ) -> Self::State {
        let erased     = world.resource::<ErasedCallbackSystem<Event, Out>>();
        let other_meta = erased.state.meta();

        if (! (
            system_meta.component_access_set().is_compatible(other_meta.component_access_set())
            && system_meta.archetype_component_access().is_compatible(other_meta.archetype_component_access())
        )) { panic!(
            "error[B0002]: A parameter in system {} (via Callback<{}, {}>) conflicts with a previous parameter in system {}. Consider removing the duplicate access. See: https://bevyengine.org/learn/errors/b0002",
            erased.system.name(),
            core::any::type_name::<Event>(),
            core::any::type_name::<Out>(),
            system_meta.name()
        ); }

        unsafe { system_meta.component_access_set_mut().extend(other_meta.component_access_set().clone()); }
        unsafe { system_meta.archetype_component_access_mut().extend(other_meta.archetype_component_access()); }
        <ResMut<'static, ErasedCallbackSystem<Event, Out>> as SystemParam>::init_state(world, system_meta)
    }

    #[inline]
    unsafe fn get_param<'w, 's>(
        state       : &'s mut Self::State,
        system_meta : &SystemMeta,
        world       : UnsafeWorldCell<'w>,
        change_tick : Tick,
    ) -> Self::Item<'w, 's> {
        let mut erased = unsafe { <ResMut<'w, ErasedCallbackSystem<Event, Out>> as SystemParam>::get_param(state, system_meta, world, change_tick) };
        erased.system.update_archetype_component_access(world);
        Callback { erased, world }
    }

    fn apply(
        _state       : &mut Self::State,
        _system_meta : &SystemMeta,
        world        : &mut World
    ) {
        let     world  = world.as_unsafe_world_cell();
        let mut erased = unsafe { world.get_resource_mut::<ErasedCallbackSystem<Event, Out>>() }.unwrap();
        erased.system.apply_deferred(unsafe { world.world_mut() });
    }
}
