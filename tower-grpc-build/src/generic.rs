use codegen;
use comments_to_rustdoc;
use prost_build;

/// Generates service code
pub struct ServiceGenerator;

impl ServiceGenerator {
    /// Generate the dummy server macro code
    pub fn generate(&self,
                    service: &prost_build::Service,
                    scope: &mut codegen::Scope) {
        self.define(service, scope);
    }

    fn define(&self,
              service: &prost_build::Service,
              scope: &mut codegen::Scope) {
        // Create scope that contains the generated server code.
        {
            scope.raw("// Starting generic::ServiceGenerator code");
            // Re-define the try_ready macro
            scope
                .raw("\
// Redefine the try_ready macro so that it doesn't need to be explicitly
// imported by the user of this generated code.
macro_rules! try_ready {
    ($e:expr) => (match $e {
        Ok(futures::Async::Ready(t)) => t,
        Ok(futures::Async::NotReady) => return Ok(futures::Async::NotReady),
        Err(e) => return Err(From::from(e)),
    })
}");


        }

        // self.define_const(service, scope);
        scope.raw(&format!("
#[macro_export]
macro_rules! {}_impl {{
    () => {{", &super::lower_name(&service.name)));
        // ---- Inside macro blob ---  //
        self.define_service_trait(service, scope);
        self.define_request_types(service, scope);
        self.impl_future_response(service, scope);
        self.define_wrapper_struct(service, scope);
        self.blanket_implementations(service, scope);
        // ---- Inside macro blob ---  //
        scope.raw("
    }
} // end macro definition");
    }

    /// Implement tower_serivce::Service for `ServiceName` and vice-versa
    fn blanket_implementations(&self,
                            service: &prost_build::Service,
                            scope: &mut codegen::Scope)
    {
        let svc_name = &service.name;
        let lower_name = ::lower_name(&service.name);
        let server_name = &format!("{}Service", svc_name);
        let req_name = &format!("{}Request", svc_name);
        let resp_name = &format!("{}Response", svc_name);
        let fut_name = &format!("{}ResponseFuture", svc_name);
        let err_name = &format!("{}Error", svc_name);

        // impl<T: Foo> Service<FooRequest> for FooService<T>
        {
            let imp = scope.new_impl(server_name)
                .generic("T")
                .target_generic("T")
                .impl_trait(&format!("tower_service::Service<{}>", req_name))
                .bound("T", svc_name)
                .associate_type("Response", resp_name)
                .associate_type("Error", err_name)
                .associate_type("Future", fut_name);

            imp.new_fn("poll_ready")
                .arg_mut_self()
                .ret("futures::Poll<(), Self::Error>")
                .line(&format!("self.{}.poll_ready()", &lower_name))
                ;

            imp.new_fn("call")
                .arg_mut_self()
                .arg("request", req_name)
                .ret("Self::Future")
                .push_block({
                    let mut match_kind = codegen::Block::new("match request");

                    for method in &service.methods {
                        let upper_name = ::to_upper_camel(&method.proto_name);

                        let match_line = format!(
                            "{}::{}(request) =>", &req_name, &upper_name
                        );

                        let mut blk = codegen::Block::new(&match_line);
                        blk
                            .line(&format!("let fut = self.{}.{}(request);", &lower_name, &method.name))
                            .line(&format!("{}::{}(Box::new(fut))", fut_name, upper_name))
                            ;

                        match_kind.push_block(blk);
                    }
                    match_kind
                })
                ;
        }

        // impl<T: Service<FooRequest>> Foo for T
        {
            let impgen = scope.new_impl("T");
            impgen
                .generic("T")
                .bound("T", &format!("tower_service::Service<{}, Response={}>", req_name, resp_name))
                .bound(&format!("<T as tower_service::Service<{}>>::Future", req_name), "Send + 'static")
                .bound(&format!("<T as tower_service::Service<{}>>::Error", req_name), "std::fmt::Debug + 'static")
                .impl_trait(svc_name);


            impgen.new_fn("poll_ready")
                .arg_mut_self()
                .ret(&format!("futures::Poll<(), {}>", err_name))
                .line(&format!("tower_service::Service::poll_ready(self).map_err({}::new)", err_name))
                ;

            for method in &service.methods {
                let name = &method.name;
                let upper_name = ::to_upper_camel(&method.proto_name);
                let input_name = &method.input_type;
                let output_name = &method.output_type;

                impgen.associate_type(&format!("{}Future", upper_name), &format!("Box<futures::Future<Item={}, Error={}> + Send>", output_name, err_name));

                let request_type = if method.client_streaming {
                    format!("Box<futures::Stream<Item={}, Error={}> + Send + 'static>", input_name, err_name)
                } else {
                    input_name.to_string()
                };

                impgen.new_fn(&name)
                    .arg_mut_self()
                    .arg("request", request_type)
                    .ret(&format!("Self::{}Future", &upper_name))
                    .doc(&comments_to_rustdoc(&method.comments))
                    .line(&format!("let fut = self.call({}::{}(request))", req_name, upper_name))
                    .line(&format!(".map_err({}::new)", err_name))
                    .push_block({
                        let mut blk = codegen::Block::new(".and_then(|resp|");
                        blk.push_block({
                            let mut blk = codegen::Block::new("let res = match resp");
                            blk.line(&format!("{}::{}(resp) => Ok(resp),", resp_name, upper_name));
                            for wrong_method in &service.methods {
                                if method.name != wrong_method.name {
                                    let wrong_name = ::to_upper_camel(&wrong_method.proto_name);
                                    blk.line(&format!("{}::{}(_) => Err({}::new(\"unexpected return type. Wanted: {}, got: {}.\")),", resp_name, wrong_name, err_name, upper_name, wrong_name));
                                }
                            }
                            blk.after(";");
                            blk
                        })
                        .line("res.into_future()")
                        .after(");");
                        blk
                    })
                    .line("Box::new(fut)");

            }
        }
    }

    /// Define wrapping `FooService<T> { inner: T }` struct.
    fn define_wrapper_struct(&self,
                            service: &prost_build::Service,
                            scope: &mut codegen::Scope)
    {
        let name = format!("{}Service", service.name);
        let lower_name = ::lower_name(&service.name);

        scope.new_struct(&name)
            .vis("pub")
            .generic("T")
            .field(&lower_name, "T")
            ;

        scope.new_impl(&name)
            .generic("T")
            .target_generic("T")
            .bound("T", &service.name)
            .new_fn("new")
            .vis("pub")
            .arg(&lower_name, "T")
            .ret("Self")
            .line(format!("Self {{ {} }}", lower_name))
            ;

        scope.new_impl(&name)
            .generic("T")
            .target_generic("T")
            .impl_trait("Clone")
            .bound("T", "Clone")
            .new_fn("clone")
            .arg_ref_self()
            .ret("Self")
            .line(&format!("Self {{ {}: self.{}.clone() }}", &lower_name, &lower_name));
    }

    ///Implement `Future` for `ServiceFutureResponse`.
    fn impl_future_response(&self,
                            service: &prost_build::Service,
                            scope: &mut codegen::Scope)
    {
        let svc_name = &service.name;
        let _req_name = &format!("{}Request", svc_name);
        let resp_name = &format!("{}Response", svc_name);
        // let resp_genname = &format!("{}<T>", resp_name);
        let fut_name = &format!("{}ResponseFuture", svc_name);
        let err_name = &format!("{}Error", svc_name);

        scope.new_impl(fut_name)
            .impl_trait("futures::Future")
            .associate_type("Item", resp_name)
            .associate_type("Error", err_name)
            .new_fn("poll")
            .arg_mut_self()
            .ret("futures::Poll<Self::Item, Self::Error>")
            .push_block({
                let mut match_kind = codegen::Block::new("match self");

                for method in &service.methods {
                    let upper_name = ::to_upper_camel(&method.proto_name);

                    let match_line = format!(
                        "{}::{}(ref mut fut) =>", &fut_name, &upper_name
                    );

                    let mut blk = codegen::Block::new(&match_line);
                    blk
                        .line("let response = fut.poll()?;")
                        .line("let response = response.map(|body| {")
                        .line(&format!("    {}::{}(body)", resp_name, &upper_name))
                        .line("});")
                        .line("Ok(response)")
                        ;

                    match_kind.push_block(blk);
                }

                match_kind
            })
            ;
        
    }

    /// Define the enums encapsulating requests/responses/futures
    fn define_request_types(&self,
                            service: &prost_build::Service,
                            scope: &mut codegen::Scope) -> (bool, bool)
    {
        let svc_name = &service.name;
        let req_name = &format!("{}Request", svc_name);
        let resp_name = &format!("{}Response", svc_name);
        let fut_name = &format!("{}ResponseFuture", svc_name);
        let err_name = &format!("{}Error", svc_name);

        let mut reqs = codegen::Enum::new(req_name);
        reqs.derive("Debug");
        reqs.derive("Clone");
        reqs.vis("pub");
        let mut resps = codegen::Enum::new(resp_name);
        resps.derive("Debug");
        resps.derive("Clone");
        resps.vis("pub");
        let mut futs = codegen::Enum::new(fut_name);
        futs.vis("pub");

        for method in &service.methods {
            let _name = &method.name;
            let upper_name = ::to_upper_camel(&method.proto_name);
            let input_name = &method.input_type;
            let output_name = &method.output_type;

            let request_type = if method.client_streaming {
                format!("Box<futures::Stream<Item={}, Error={}> + Send + 'static>", input_name, err_name)
            } else {
                input_name.to_string()
            };
            let response_type = if method.server_streaming {
                format!("Box<futures::Stream<Item={}, Error={}> + Send + 'static>", output_name, err_name)
            } else {
                output_name.to_string()
            };
            let fut_type = format!("Box<futures::Future<Item={}, Error={}> + Send + 'static>", response_type, err_name);

            reqs.new_variant(&format!("{}({})", upper_name, request_type));
            resps.new_variant(&format!("{}({})", upper_name, response_type));
            futs.new_variant(&format!("{}({})", upper_name, fut_type));
        }

        scope.push_enum(reqs);
        scope.push_enum(resps);
        scope.push_enum(futs);

        let path = format!("\"{}.{}\"",
                service.package,
                service.proto_name);

        scope.new_impl(req_name)
            .impl_trait("Request")
            .associate_type("Response", resp_name)
            .associate_type("Future", fut_name)
            // TODO: Please don't judge, I will PR upstream for associated constants one day...
            .associate_type(&format!("Error = {};\n\nconst PATH: &'static str", err_name), path);

        (true, true)
    }
    /// Define the generic service trait
    /// Where the input/output parameters are left undefined
    ///
    /// These can be set to prost::* types if necessary.
    fn define_service_trait(&self,
                            service: &prost_build::Service,
                            scope: &mut codegen::Scope)
    {
        let svc_name = &service.name;
        let trait_name = svc_name;
        let err_name = &format!("{}Error", svc_name);

        let mut service_trait = codegen::Trait::new(trait_name);
        service_trait.vis("pub")
            // .parent("Clone")
            .doc(&comments_to_rustdoc(&service.comments))
            ;

        for method in &service.methods {
            let name = &method.name;
            let upper_name = ::to_upper_camel(&method.proto_name);
            let future_bound = if method.server_streaming {
                // let stream_name = format!("{}Stream", &upper_name);
                // let stream_bound = format!(
                //     "futures::Stream<Item = {}, Error = {}> + Send",
                //     &method.output_type, err_name);
                format!(
                    "futures::Future<Item= {}, Error={}> + Send",
                    format!("Box<futures::Stream<Item={}, Error={}> + Send + 'static>", &method.output_type, err_name),
                    err_name)

                // service_trait.associated_type(&stream_name)
                    // .bound(&stream_bound);
            } else {
                // 
                format!(
                    "futures::Future<Item = {}, Error = {}> + Send + 'static",
                    &method.output_type,
                    err_name)
            };

            let future_name = format!("{}Future", &upper_name);

            service_trait.associated_type(&future_name)
                .bound(&future_bound)
                ;

            let input_type = &method.input_type;

            let request_type = if method.client_streaming {
                // let stream_name = format!("{}Stream", input_type);
                format!("Box<futures::Stream<Item={}, Error={}> + Send + 'static>", input_type, err_name)
                // service_trait.associated_type(&stream_name)
                //     .bound(&stream_bound);
                // format!("Streaming<{}>", input_type)
                // format!("Self::{}", stream_name)
            } else {
                input_type.to_string()
            };

            service_trait.new_fn(&name)
                .arg_mut_self()
                .arg("request", request_type)
                .ret(&format!("Self::{}Future", &upper_name))
                .doc(&comments_to_rustdoc(&method.comments))
                ;

        }
        service_trait.new_fn("poll_ready")
            .arg_mut_self()
            .ret(&format!("futures::Poll<(), {}>",err_name))
            .line("Ok(futures::Async::Ready(()))")
            ;

        scope.push_trait(service_trait);
    }
}