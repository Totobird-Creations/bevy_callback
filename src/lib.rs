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
        Req,
        Callback,
        OptionCallback
    };
}


#[derive(Resource)]
pub struct ErasedCallbackSystem<E, Out> {
    system : Box<dyn System<In = Req<E>, Out = Out> + Send + Sync>,
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
    fn add_callback<E, S, M>(&mut self, system : S) -> &mut Self
    where
        E : Request + 'static,
        S : IntoCallbackSystem<E, E::Response, M>;
}
#[cfg(feature = "app")]
impl AppExt for bevy_app::App {
    fn add_callback<E, S, M>(&mut self, system : S) -> &mut Self
    where
        E : Request + 'static,
        S : IntoCallbackSystem<E, E::Response, M>
    {
        let     world  = self.world_mut();
        let mut system = IntoCallbackSystem::into_system(system);
        let     state  = SystemState::<<S::System as ParametisedSystem>::Param>::new(world);

        let world = self.world_mut();
        if (world.contains_resource::<ErasedCallbackSystem<E, E::Response>>()) {
            panic!(
                "Duplicate ({}) -> {} callback registered",
                core::any::type_name::<E>(),
                core::any::type_name::<E::Response>()
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


pub trait Request {
    type Response;
}


#[derive(Debug)]
pub struct Req<E>(pub E);

impl<E> Deref for Req<E> {
    type Target = E;
    #[inline]
    fn deref(&self) -> &Self::Target { &self.0 }
}
impl<E> DerefMut for Req<E> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.0 }
}

impl<E> SystemInput for Req<E>
where
    E : 'static
{
    type Param<'i> = Req<E>;
    type Inner<'i> = Req<E>;
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


pub trait CallbackSystem<E, Out>
where
    Self : ParametisedSystem + System<In = Req<E>>
{ }

impl<E, A, B> CallbackSystem<E, B::Out> for PipeSystem<A, B>
where
            A     : ParametisedSystem<In = Req<E>>,
            B     : ParametisedSystem,
    for<'l> B::In : SystemInput<Inner<'l> = A::Out>
{ }

impl<E, A, B, F> CallbackSystem<E, F::Out> for CombinatorSystem<F, A, B>
where
    F : Combine<A, B, In = Req<E>> + 'static,
    A : ParametisedSystem,
    B : ParametisedSystem
{ }

impl<E, F, S> CallbackSystem<E, F::Out> for AdapterSystem<F, S>
where
    F : Adapt<S, In = Req<E>>,
    S : ParametisedSystem
{ }

impl<E, M, F> CallbackSystem<E, F::Out> for FunctionSystem<M, F>
where
    M : 'static,
    F : SystemParamFunction<M, In = Req<E>>
{
}


pub trait IntoCallbackSystem<E, Out, M> {
    type System : CallbackSystem<E, Out>;
    fn into_system(this : Self) -> Self::System;
}

impl<E, Out, S, M> IntoCallbackSystem<E, Out, M> for S
where
    S         : IntoSystem<Req<E>, Out, M>,
    S::System : CallbackSystem<E, Out>,
    E         : 'static
{
    type System = S::System;
    #[inline]
    fn into_system(this : Self) -> Self::System { S::into_system(this) }
}


pub struct Callback<'w, E>
where
    E : Request + 'static
{
    erased : ResMut<'w, ErasedCallbackSystem<E, E::Response>>,
    world  : UnsafeWorldCell<'w>
}

impl<E> Callback<'_, E>
where
    E : Request
{
    pub fn request(&mut self, req : E) -> E::Response {
        unsafe { self.erased.system.run_unsafe(Req(req), self.world) }
    }
}

unsafe impl<E> SystemParam for Callback<'_, E>
where
    E : Request
{
    type State        = ComponentId;
    type Item<'w, 's> = Callback<'w, E>;

    fn init_state(
        world       : &mut World,
        system_meta : &mut SystemMeta
    ) -> Self::State {
        let erased     = world.resource::<ErasedCallbackSystem<E, E::Response>>();
        let other_meta = erased.state.meta();

        if (! (
            system_meta.component_access_set().is_compatible(other_meta.component_access_set())
            && system_meta.archetype_component_access().is_compatible(other_meta.archetype_component_access())
        )) { panic!(
            "error[B0002]: A parameter in system {} (via Callback<{}>) conflicts with a previous parameter in system {}. Consider removing the duplicate access. See: https://bevyengine.org/learn/errors/b0002",
            erased.system.name(),
            core::any::type_name::<E>(),
            system_meta.name()
        ); }

        unsafe { system_meta.component_access_set_mut().extend(other_meta.component_access_set().clone()); }
        unsafe { system_meta.archetype_component_access_mut().extend(other_meta.archetype_component_access()); }
        <ResMut<'static, ErasedCallbackSystem<E, E::Response>> as SystemParam>::init_state(world, system_meta)
    }

    #[inline]
    unsafe fn get_param<'w, 's>(
        state       : &'s mut Self::State,
        system_meta : &SystemMeta,
        world       : UnsafeWorldCell<'w>,
        change_tick : Tick,
    ) -> Self::Item<'w, 's> {
        let mut erased = unsafe { <ResMut<'w, ErasedCallbackSystem<E, E::Response>> as SystemParam>::get_param(state, system_meta, world, change_tick) };
        erased.system.update_archetype_component_access(world);
        Callback { erased, world }
    }

    fn apply(
        _state       : &mut Self::State,
        _system_meta : &SystemMeta,
        world        : &mut World
    ) {
        let     world  = world.as_unsafe_world_cell();
        let mut erased = unsafe { world.get_resource_mut::<ErasedCallbackSystem<E, E::Response>>() }.unwrap();
        erased.system.apply_deferred(unsafe { world.world_mut() });
    }
}


pub struct OptionCallback<'w, E>(pub Option<Callback<'w, E>>)
where
    E : Request + 'static;

impl<'w, E> Deref for OptionCallback<'w, E>
where
    E : Request + 'static
{
    type Target = Option<Callback<'w, E>>;
    fn deref(&self) -> &Self::Target { &self.0 }
}
impl<'w, E> DerefMut for OptionCallback<'w, E>
where
    E : Request + 'static
{
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.0 }
}

unsafe impl<E> SystemParam for OptionCallback<'_, E>
where
    E : Request
{
    type State        = Option<ComponentId>;
    type Item<'w, 's> = OptionCallback<'w, E>;

    fn init_state(
        world       : &mut World,
        system_meta : &mut SystemMeta
    ) -> Self::State {
        if let Some(erased) = world.get_resource::<ErasedCallbackSystem<E, E::Response>>() {
            let other_meta = erased.state.meta();

            if (! (
                system_meta.component_access_set().is_compatible(other_meta.component_access_set())
                && system_meta.archetype_component_access().is_compatible(other_meta.archetype_component_access())
            )) { panic!(
                "error[B0002]: A parameter in system {} (via Callback<{}>) conflicts with a previous parameter in system {}. Consider removing the duplicate access. See: https://bevyengine.org/learn/errors/b0002",
                erased.system.name(),
                core::any::type_name::<E>(),
                system_meta.name()
            ); }

            unsafe { system_meta.component_access_set_mut().extend(other_meta.component_access_set().clone()); }
            unsafe { system_meta.archetype_component_access_mut().extend(other_meta.archetype_component_access()); }
            Some(<ResMut<'static, ErasedCallbackSystem<E, E::Response>> as SystemParam>::init_state(world, system_meta))
        } else { None }
    }

    #[inline]
    unsafe fn get_param<'w, 's>(
        state       : &'s mut Self::State,
        system_meta : &SystemMeta,
        world       : UnsafeWorldCell<'w>,
        change_tick : Tick,
    ) -> Self::Item<'w, 's> {
        OptionCallback(if let Some(state) = state {
            let mut erased = unsafe { <ResMut<'w, ErasedCallbackSystem<E, E::Response>> as SystemParam>::get_param(state, system_meta, world, change_tick) };
            erased.system.update_archetype_component_access(world);
            Some(Callback { erased, world })
        } else { None })
    }

    fn apply(
        state        : &mut Self::State,
        _system_meta : &SystemMeta,
        world        : &mut World
    ) {
        if let Some(_) = state {
            let     world  = world.as_unsafe_world_cell();
            let mut erased = unsafe { world.get_resource_mut::<ErasedCallbackSystem<E, E::Response>>() }.unwrap();
            erased.system.apply_deferred(unsafe { world.world_mut() });
        }
    }
}
